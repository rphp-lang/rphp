mod common;

use common::run_php;

#[test]
fn literal_disallowed_codepoints_follow_each_document_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$source = pack(
    'C*',
    0, 12, 13, 127,
    0xC2, 0x80,
    0xEF, 0xB7, 0x90,
    0xEF, 0xBF, 0xBE,
    0xF0, 0x9F, 0xBF, 0xBE
);
foreach ([
    'h4' => ENT_HTML401,
    'xhtml' => ENT_XHTML,
    'h5' => ENT_HTML5,
    'xml' => ENT_XML1,
] as $name => $document) {
    echo $name, '/entities=', bin2hex(htmlentities(
        $source,
        $document | ENT_DISALLOWED,
        'UTF-8'
    )), "\n";
    echo $name, '/special=', bin2hex(htmlspecialchars(
        $source,
        $document | ENT_DISALLOWED,
        'UTF-8'
    )), "\n";
}
"#,
        ),
        concat!(
            "h4/entities=efbfbdefbfbd0defbfbdefbfbdefb790efbfbef09fbfbe\n",
            "h4/special=efbfbdefbfbd0defbfbdefbfbdefb790efbfbef09fbfbe\n",
            "xhtml/entities=efbfbdefbfbd0d7fc280efb790efbfbdf09fbfbe\n",
            "xhtml/special=efbfbdefbfbd0d7fc280efb790efbfbdf09fbfbe\n",
            "h5/entities=efbfbd0c0defbfbdefbfbdefbfbdefbfbdefbfbd\n",
            "h5/special=efbfbd0c0defbfbdefbfbdefbfbdefbfbdefbfbd\n",
            "xml/entities=efbfbdefbfbd0d7fc280efb790efbfbdf09fbfbe\n",
            "xml/special=efbfbdefbfbd0d7fc280efb790efbfbdf09fbfbe\n",
        )
    );
}

#[test]
fn double_encode_false_validates_numeric_references_before_preserving_them() {
    assert_eq!(
        run_php(
            r#"<?php
$source = '&#0;|&#x0C;|&#x0D;|&#xD800;|&#xFDD0;|&#x1FFFE;|&#x110000;|&bogus;';
foreach ([
    'h4' => ENT_HTML401,
    'xhtml' => ENT_XHTML,
    'h5' => ENT_HTML5,
    'xml' => ENT_XML1,
] as $name => $document) {
    foreach (['htmlentities', 'htmlspecialchars'] as $function) {
        echo $name, '/', $function, '=', $function(
            $source,
            $document | ENT_DISALLOWED,
            'UTF-8',
            false
        ), "\n";
    }
}
"#,
        ),
        concat!(
            "h4/htmlentities=&#0;|&#x0C;|&#x0D;|&#xD800;|&#xFDD0;|&#x1FFFE;|&amp;#x110000;|&amp;bogus;\n",
            "h4/htmlspecialchars=&#0;|&#x0C;|&#x0D;|&#xD800;|&#xFDD0;|&#x1FFFE;|&amp;#x110000;|&amp;bogus;\n",
            "xhtml/htmlentities=&amp;#0;|&amp;#x0C;|&#x0D;|&amp;#xD800;|&#xFDD0;|&#x1FFFE;|&amp;#x110000;|&amp;bogus;\n",
            "xhtml/htmlspecialchars=&amp;#0;|&amp;#x0C;|&#x0D;|&amp;#xD800;|&#xFDD0;|&#x1FFFE;|&amp;#x110000;|&amp;bogus;\n",
            "h5/htmlentities=&amp;&num;0&semi;&vert;&#x0C;&vert;&amp;&num;x0D&semi;&vert;&#xD800;&vert;&amp;&num;xFDD0&semi;&vert;&amp;&num;x1FFFE&semi;&vert;&amp;&num;x110000&semi;&vert;&amp;bogus&semi;\n",
            "h5/htmlspecialchars=&amp;#0;|&#x0C;|&amp;#x0D;|&#xD800;|&amp;#xFDD0;|&amp;#x1FFFE;|&amp;#x110000;|&amp;bogus;\n",
            "xml/htmlentities=&amp;#0;|&amp;#x0C;|&#x0D;|&amp;#xD800;|&#xFDD0;|&#x1FFFE;|&amp;#x110000;|&amp;bogus;\n",
            "xml/htmlspecialchars=&amp;#0;|&amp;#x0C;|&#x0D;|&amp;#xD800;|&#xFDD0;|&#x1FFFE;|&amp;#x110000;|&amp;bogus;\n",
        )
    );
}

#[test]
fn legacy_encodings_replace_unrepresentable_values_and_validate_sjis_units() {
    assert_eq!(
        run_php(
            r#"<?php
$source = pack('C*', 0, 12, 127, 128, 159, 160);
foreach (['h4' => ENT_HTML401, 'h5' => ENT_HTML5] as $name => $document) {
    echo $name, '/entities=', bin2hex(htmlentities(
        $source,
        $document | ENT_DISALLOWED,
        'Windows-1251'
    )), "\n";
    echo $name, '/special=', bin2hex(htmlspecialchars(
        $source,
        $document | ENT_DISALLOWED,
        'Windows-1251'
    )), "\n";
}
set_error_handler(function ($level, $message) {
    echo 'diag=', $level, ':', $message, "\n";
    return true;
});
$invalid = htmlentities(chr(0x80), ENT_HTML5 | ENT_DISALLOWED, 'SJIS');
echo 'invalid=', bin2hex($invalid), "\n";
$reference = htmlentities('&#x0D;', ENT_HTML5 | ENT_DISALLOWED, 'SJIS', false);
echo 'reference=', $reference, "\n";
"#,
        ),
        concat!(
            "h4/entities=262378464646443b262378464646443b262378464646443b809f266e6273703b\n",
            "h4/special=262378464646443b262378464646443b7f809fa0\n",
            "h5/entities=262378464646443b0c262378464646443b26444a63793b26647a63793b266e6273703b\n",
            "h5/special=262378464646443b0c7f809fa0\n",
            "diag=8:htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars\n",
            "invalid=\n",
            "diag=8:htmlentities(): Only basic entities substitution is supported for multi-byte encodings other than UTF-8; functionality is equivalent to htmlspecialchars\n",
            "reference=&amp;#x0D;\n",
        )
    );
}
