use exiftool_rs::tags::conv_expr::{eval, Val};
fn main() {
    for e in [
        "$val =~ /^..([a-z0-9]{4})/i ? hex($1) : undef",
        "$val=~/(\\d+) (\\d+)/ ? \"$2.$1\" : \"0.$val\"",
        "(($val >> 13) & 0x7) . \" \" . (($val >> 12) & 0x1)",
        "$val m",
        "$_=join(\".\", unpack(\"C*\", $val))); s/(:.*?:.*?:.*?):/$1 /; $_",
        "Get8u(\\$val,0) . \": \" . substr($val, 1)",
    ] {
        println!("{:?} -> {:?}", e, eval(e, &Val::Str("ab12345".into())));
    }
}
