#!/usr/bin/env perl
# Round-trips the DDL corpus in corpus.json (extracted from PostgreSQL's own
# pg_dump test suite, see extract.pl) through tursopg and a real pg_dump,
# and reports what fraction of each construct's expected pg_dump output
# actually appears.
#
# For each corpus entry, in create_order:
#   1. Apply create_sql to a running tursopg server over the wire protocol.
#   2. After all entries have been attempted, run a real `pg_dump
#      --schema-only` against that same server.
#   3. For every entry that executed without error, check whether its
#      `regexp` (verbatim from pg_dump's own test suite -- the pattern real
#      PostgreSQL's tests require to appear in a correct dump) matches
#      somewhere in the dump output.
#
# This measures two distinct things, reported separately:
#   - exec rate:  fraction of DDL constructs tursopg accepts at all.
#   - dump rate:  of those accepted, fraction whose catalog-driven pg_dump
#                 representation matches what real PostgreSQL requires.
# Conflating them into one number would hide which of "doesn't parse/apply"
# vs "applies but the catalog emulation misrepresents it" is the bottleneck.
#
# Usage:
#   perl check.pl                  # run the whole corpus
#   perl check.pl --filter TABLE   # only entries whose name contains TABLE
#   perl check.pl --keep           # keep the temp dir + dump for inspection

use strict;
use warnings;
use FindBin qw($RealBin);
use File::Temp qw(tempdir tempfile);
use JSON::PP qw(decode_json encode_json);
use IO::Socket::INET;
use Time::HiRes qw(sleep time);

my $ROOT = "$RealBin/../../..";

my %opt = (filter => undef, keep => 0);
for (my $i = 0; $i <= $#ARGV; $i++) {
    if ($ARGV[$i] eq '--filter') { $opt{filter} = $ARGV[++$i]; }
    elsif ($ARGV[$i] eq '--keep') { $opt{keep} = 1; }
    else { die "unknown argument: $ARGV[$i]\n"; }
}

# ---------------------------------------------------------------------------
# Load the corpus
# ---------------------------------------------------------------------------

open(my $cfh, '<', "$RealBin/corpus.json") or die "reading corpus.json: $!\n";
local $/ = undef;
my @corpus = @{ decode_json(<$cfh>) };
close $cfh;

if (defined $opt{filter}) {
    @corpus = grep { index($_->{name}, $opt{filter}) >= 0 } @corpus;
    die "no corpus entries match --filter '$opt{filter}'\n" unless @corpus;
}

# ---------------------------------------------------------------------------
# Build and start tursopg
# ---------------------------------------------------------------------------

system('cargo', 'build', '--manifest-path', "$ROOT/Cargo.toml", '--package', 'tursopg') == 0
    or die "cargo build -p tursopg failed\n";

my $target_dir = $ENV{CARGO_TARGET_DIR} // "$ROOT/target";
my $profile    = $ENV{CI} ? 'release' : 'debug';
my $tursopg    = "$target_dir/$profile/tursopg";
die "tursopg binary not found at $tursopg\n" unless -x $tursopg;

sub free_port {
    my $sock = IO::Socket::INET->new(LocalAddr => '127.0.0.1', LocalPort => 0, Listen => 1);
    my $port = $sock->sockport;
    $sock->close;
    return $port;
}

my $port = free_port();
my $tmp  = tempdir(CLEANUP => !$opt{keep});
print "workdir: $tmp\n" if $opt{keep};

my $pid = fork();
die "fork failed: $!\n" unless defined $pid;
if ($pid == 0) {
    open(STDOUT, '>', "$tmp/server.out") or die;
    open(STDERR, '>', "$tmp/server.err") or die;
    exec($tursopg, '--server', "127.0.0.1:$port", "$tmp/dumpddl.db")
        or die "exec tursopg failed: $!\n";
}

my $deadline = time() + 15;
my $up = 0;
while (time() < $deadline) {
    if (my $s = IO::Socket::INET->new(PeerAddr => '127.0.0.1', PeerPort => $port, Timeout => 1)) {
        $s->close;
        $up = 1;
        last;
    }
    sleep(0.05);
}
unless ($up) {
    kill('TERM', $pid);
    die "tursopg did not accept connections on port $port within 15s\n";
}

my $dsn = "postgres://127.0.0.1:$port/dumpddl";

END {
    if (defined $pid && $pid) {
        kill('TERM', $pid);
        waitpid($pid, 0);
    }
}

# ---------------------------------------------------------------------------
# Apply each entry's create_sql
# ---------------------------------------------------------------------------

my @results;
for my $entry (@corpus) {
    my ($fh, $filename) = tempfile(SUFFIX => '.sql', DIR => $tmp);
    print $fh $entry->{create_sql};
    close $fh;

    my $err = "$filename.err";
    my $rc  = system("psql '$dsn' -v ON_ERROR_STOP=1 -q -f '$filename' >/dev/null 2>'$err'");
    my $exec_ok = ($rc == 0);
    my $err_text = '';
    if (!$exec_ok) {
        open(my $efh, '<', $err) or die;
        local $/ = undef;
        $err_text = <$efh>;
        close $efh;
        chomp $err_text;
    }
    push @results, {
        name     => $entry->{name},
        regexp   => $entry->{regexp},
        exec_ok  => $exec_ok,
        exec_err => $err_text,
    };
}

# ---------------------------------------------------------------------------
# One real pg_dump over the accumulated schema
# ---------------------------------------------------------------------------

my $dump_text = `pg_dump '$dsn' --schema-only --no-owner --no-privileges 2>"$tmp/pg_dump.err"`;
if (defined $opt{keep}) {
    open(my $dfh, '>', "$tmp/schema.dump") or die;
    print $dfh $dump_text;
    close $dfh;
}

for my $r (@results) {
    next unless $r->{exec_ok};
    if (!defined $r->{regexp}) {
        $r->{dump_status} = 'no_assertion';
        next;
    }
    my $pat = $r->{regexp};
    $r->{dump_status} = ($dump_text =~ /$pat/) ? 'match' : 'mismatch';
}

# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

my $total          = scalar @results;
my $exec_ok        = grep { $_->{exec_ok} } @results;
my $dump_match     = grep { ($_->{dump_status} // '') eq 'match' } @results;
my $dump_checkable = grep { $_->{exec_ok} && defined $_->{regexp} } @results;

for my $r (@results) {
    my $status = !$r->{exec_ok}          ? 'EXEC_FAIL'
               : ($r->{dump_status} // '') eq 'mismatch' ? 'DUMP_MISMATCH'
               : ($r->{dump_status} // '') eq 'no_assertion' ? 'OK (no assertion)'
               :                            'OK';
    printf "%-14s %s\n", $status, $r->{name};
    if (!$r->{exec_ok} && $r->{exec_err}) {
        (my $first_line = $r->{exec_err}) =~ s/\n.*//s;
        print "               $first_line\n";
    }
}

printf "\n%d total DDL constructs (source: PostgreSQL's own pg_dump test suite)\n", $total;
printf "  exec accepted:      %3d / %-3d (%.1f%%)\n", $exec_ok, $total, 100 * $exec_ok / $total;
printf "  dump matches spec:  %3d / %-3d (%.1f%%) of accepted constructs with an assertion\n",
    $dump_match, $dump_checkable, $dump_checkable ? 100 * $dump_match / $dump_checkable : 0;
printf "  overall compliance: %3d / %-3d (%.1f%%)\n", $dump_match, $total, 100 * $dump_match / $total;

my $report_path = "$RealBin/results.json";
open(my $rfh, '>', $report_path) or die "writing $report_path: $!\n";
print $rfh JSON::PP->new->canonical->pretty->encode({
    total          => $total,
    exec_ok        => $exec_ok,
    dump_checkable => $dump_checkable,
    dump_match     => $dump_match,
    results        => \@results,
});
close $rfh;
print "\nwrote $report_path\n";
