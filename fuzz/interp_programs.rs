// The programs of the `interp` target and how an input runs them: included
// by fuzz_targets/interp.rs and by the port's tests/fuzz_regressions.rs,
// and read by interp_perl.pl, which runs an input the same way in perl.
//
// The first input byte picks the program; the rest is split on 0xFF into
// the operands `$a`, `$b` and `$c` (missing ones are empty). Each program
// runs in `eval { }`, so a Perl `die` ends it quietly.

const PROGRAMS: &[&str] = &[
    // regex on bytes: match, list match, /g, s///, split, pos and \G
    r#"my @m = ($b =~ /$a/s); my @g = ($b =~ /$a/g); (my $x = $b) =~ s/$a/<$1>/g;
       my @p = split /$a/, $b; my $n = () = $b =~ /\G$a/gc; my $q = qr/$a/i; $b =~ $q; "$q";"#,
    // regex on character strings, with the other flags
    r#"utf8::decode($a); utf8::decode($b); my @m = ($b =~ /$a/imx); (my $x = $b) =~ s/$a/uc($&)/ge;
       my @g = ($b =~ /$a/gs); my $p = pos($b); my @l = map { length } @g; lc($b) =~ /\Q$a\E/;"#,
    // unpack then pack with the same template, then pack from text items
    r#"my @v = unpack($a, $b); my $r = pack($a, @v); my @w = split /,/, $c; my $s = pack($a, @w);
       my $n = unpack("%32C*", $r); my @u = unpack($a, $s);"#,
    // sprintf with a fuzzed format and text and number arguments
    r#"my @w = split /,/, $b; my $r = sprintf($a, @w); my $s = sprintf($a, map { $_ + 0 } @w);
       my $t = sprintf($a, unpack("d*", $c . "\0" x 8)); my $v = sprintf("%vd", $b);"#,
    // tr (and y) with fuzzed search and replacement lists, by string eval
    r#"s{[/\\]+}{}g, s{\\$}{} for $a, $c; (my $f = $c) =~ tr/cdsr//cd;
       my $x = $b; my $n = eval "\$x =~ tr/$a/$c/$f"; utf8::decode(my $y = $b); eval "\$y =~ y/$a/$c/$f";
       my $k = ($b =~ tr/\0-\x1f//);"#,
    // substr (rvalue, lvalue, 4-argument), index, rindex, with fuzzed offsets
    r#"my ($o, $l) = unpack("c c", $c . "\0\0"); my $x = $b; my $s = substr($x, $o, $l);
       substr($x, $o, $l) = $a; substr($x, $o, $l, uc $a); my $i = index($b, $a, $o);
       my $r = rindex($b, $a, $l); utf8::decode(my $u = $b); my $t = substr($u, $o); substr($u, $l, 1) = "\x{263a}";
       my $v = substr($b, $o, $l); vec($x, $o & 63, 8) = $l;"#,
    // utf8 and Encode: decode, encode, upgrade, downgrade, ord/chr, case
    r#"require Encode; my $x = $b; utf8::decode($x); utf8::encode(my $e = $x); utf8::upgrade(my $u = $b);
       my $d = $u; utf8::downgrade($d, 1); my $s = Encode::decode('UTF-8', $b);
       my $t = Encode::encode('UTF-16LE', $x); my $w = Encode::decode('UTF-16BE', $b);
       my $l = Encode::decode('Latin1', $b); my $o = join ',', map { ord } split //, $x;
       my $c2 = join '', map { chr } unpack("n*", $b); my $uc = uc($x) . lc($x) . ucfirst($x);
       my $p = pack("U0C*", unpack("C*", $b)); my $q = pack("C0U*", unpack("C*", $a)); length($p);"#,
    // numbers from text: numeric context, hex, oct, int, formatting
    r#"my $n = $b + 0; my $h = hex($a); my $o = oct($b); my $i = int($b * 1.5);
       my $f = sprintf("%.15g %d %x %e", $b, $b, $a, $b); my $s = $b ** 0.5; my $m = $b % ($a || 1);
       my $z = $b x (length($a) % 8); my $c2 = $b <=> $a; my $j = join ',', sort split //, $b;"#,
];

/// Run an input in a new interpreter. Returns `$@` as the program left it
/// (empty when it ended normally).
pub fn run_interp_input(data: &[u8]) -> Vec<u8> {
    use exiftool_port::interp::{Scope, Want};
    use exiftool_port::perl::Value;
    let Some((&pick, body)) = data.split_first() else {
        return Vec::new();
    };
    let program = PROGRAMS[pick as usize % PROGRAMS.len()];
    let mut parts = body.splitn(3, |&b| b == 0xff);
    let mut it = exiftool_port::runtime::new_interp();
    it.strict = false;
    let scope = Scope::new(None);
    for name in ["a", "b", "c"] {
        scope.scalar(name, Value::bytes(parts.next().unwrap_or_default()));
    }
    let code = format!("eval {{ {program} }}; $@;");
    match it.eval_string(code.as_bytes(), "main", &scope, Want::Scalar) {
        Ok(v) => v.first().map(|v| v.to_bytes().to_vec()).unwrap_or_default(),
        Err(e) => e.to_bytes().to_vec(),
    }
}
