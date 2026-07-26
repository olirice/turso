#!/usr/bin/env perl
# Extracts DDL fixtures from PostgreSQL's own pg_dump test suite
# (upstream/002_pg_dump.pl, imported verbatim from
# src/bin/pg_dump/t/002_pg_dump.pl, pinned to REL_16_STABLE) for use as a
# round-trip corpus against tursopg.
#
# 002_pg_dump.pl defines a %tests hash: one entry per DDL construct, each
# carrying the SQL that creates it (create_sql) and the regexp pg_dump's own
# test suite requires to appear in a --schema-only dump of that construct
# (regexp). We only take entries tagged `section_pre_data` in their `like`
# set — the schema-defining DDL objects — not data/ACL/option-flag tests.
#
# The %tests hash only depends on %full_runs and %dump_test_schema_runs
# (plain literal hashes defined just above it); it does not depend on the
# PostgreSQL::Test::Cluster machinery that runs later in the file, so we can
# slice out just those three hash literals and eval them standalone.
#
# Usage:
#   perl extract.pl upstream/002_pg_dump.pl > corpus.json

use strict;
use warnings;
use JSON::PP qw(encode_json);

my $path = shift @ARGV or die "usage: extract.pl <path-to-002_pg_dump.pl>\n";
open(my $fh, '<', $path) or die "reading $path: $!\n";
local $/ = undef;
my $source = <$fh>;
close $fh;

my $start_marker = 'my %dump_test_schema_runs = (';
my $end_marker    = "my \$node = PostgreSQL::Test::Cluster->new";

my $start = index($source, $start_marker);
my $end   = index($source, $end_marker);
die "could not locate %tests block in $path (markers not found)\n"
    if $start < 0 || $end < 0 || $end <= $start;

my $hash_literals = substr($source, $start, $end - $start);

# Everything below runs inside the same eval as $hash_literals, so it shares
# its lexical %tests, %full_runs, %dump_test_schema_runs.
my $extract_and_dump = <<'PERL';
my @rows;
for my $name (sort keys %tests) {
    my $t = $tests{$name};
    next unless defined $t->{create_sql};
    next unless $t->{like} && exists $t->{like}{section_pre_data};
    push @rows, {
        name         => $name,
        create_order => $t->{create_order},
        create_sql   => $t->{create_sql},
        regexp       => defined $t->{regexp} ? "$t->{regexp}" : undef,
        collation    => $t->{collation} ? JSON::PP::true : JSON::PP::false,
        icu          => $t->{icu} ? JSON::PP::true : JSON::PP::false,
    };
}
@rows = sort {
    ($a->{create_order} // 1e9) <=> ($b->{create_order} // 1e9)
        or $a->{name} cmp $b->{name}
} @rows;
print JSON::PP->new->canonical->pretty->encode(\@rows);
PERL

my $program = "use strict;\nuse warnings;\n" . $hash_literals . "\n" . $extract_and_dump;
eval $program;
die "extraction failed: $@" if $@;
