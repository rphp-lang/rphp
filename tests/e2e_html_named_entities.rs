mod common;

use common::run_php;

#[test]
fn encoding_uses_document_canonical_names_longest_matches_and_double_encode() {
    assert_eq!(
        run_php(
            r#"<?php
$source = "\"&'<> ∵!fj<\u{20D2}&Because;&AMP;";
foreach ([
    ['h4', ENT_HTML401],
    ['xml', ENT_XML1],
    ['xhtml', ENT_XHTML],
    ['h5', ENT_HTML5],
] as [$name, $document]) {
    echo $name, '=', htmlentities(
        $source,
        ENT_QUOTES | $document,
        'UTF-8',
        false
    ), "\n";
}
"#,
        ),
        concat!(
            "h4=&quot;&amp;&#039;&lt;&gt;&nbsp;∵!fj&lt;⃒&amp;Because;&amp;AMP;\n",
            "xml=&quot;&amp;&apos;&lt;&gt; ∵!fj&lt;⃒&amp;Because;&amp;AMP;\n",
            "xhtml=&quot;&amp;&#039;&lt;&gt;&nbsp;∵!fj&lt;⃒&amp;Because;&amp;AMP;\n",
            "h5=&quot;&amp;&apos;&lt;&gt;&nbsp;&Because;&excl;&fjlig;&nvlt;&Because;&AMP;\n",
        )
    );
}

#[test]
fn decoding_accepts_document_aliases_multicodepoint_values_and_quote_modes() {
    assert_eq!(
        run_php(
            r#"<?php
$source = '&AMP;&AElig;&nparsl;&fjlig;&apos;&Alpha;&we;';
foreach ([
    ['h4', ENT_HTML401],
    ['xml', ENT_XML1],
    ['xhtml', ENT_XHTML],
    ['h5', ENT_HTML5],
] as [$name, $document]) {
    foreach ([ENT_QUOTES, ENT_COMPAT, ENT_NOQUOTES] as $quotes) {
        $decoded = html_entity_decode($source, $quotes | $document, 'UTF-8');
        echo $name, '/', $quotes, '=', bin2hex($decoded), "\n";
    }
}
"#,
        ),
        concat!(
            "h4/3=26414d503bc386266e706172736c3b26666a6c69673b2661706f733bce912677653b\n",
            "h4/2=26414d503bc386266e706172736c3b26666a6c69673b2661706f733bce912677653b\n",
            "h4/0=26414d503bc386266e706172736c3b26666a6c69673b2661706f733bce912677653b\n",
            "xml/3=26414d503b2641456c69673b266e706172736c3b26666a6c69673b2726416c7068613b2677653b\n",
            "xml/2=26414d503b2641456c69673b266e706172736c3b26666a6c69673b2661706f733b26416c7068613b2677653b\n",
            "xml/0=26414d503b2641456c69673b266e706172736c3b26666a6c69673b2661706f733b26416c7068613b2677653b\n",
            "xhtml/3=26414d503bc386266e706172736c3b26666a6c69673b27ce912677653b\n",
            "xhtml/2=26414d503bc386266e706172736c3b26666a6c69673b2661706f733bce912677653b\n",
            "xhtml/0=26414d503bc386266e706172736c3b26666a6c69673b2661706f733bce912677653b\n",
            "h5/3=26c386e2abbde283a5666a27ce912677653b\n",
            "h5/2=26c386e2abbde283a5666a2661706f733bce912677653b\n",
            "h5/0=26c386e2abbde283a5666a2661706f733bce912677653b\n",
        )
    );
}

#[test]
fn valid_utf8_byte_results_interoperate_with_tables_concat_and_byte_consumers() {
    assert_eq!(
        run_php(
            r#"<?php
$key = pack('C2', 0xc2, 0xa0);
$table = get_html_translation_table(HTML_ENTITIES, ENT_QUOTES, 'UTF-8');
echo isset($table[$key]) ? 'yes' : 'no', ':', $table[$key], ':', count($table), "\n";
$table[$key] = 'updated';
echo $table[" "], ':', array_key_exists($key, $table) ? 'yes' : 'no', "\n";
unset($table[$key]);
echo count($table), "\n";
$decoded = html_entity_decode('&AElig;&nparsl;', ENT_QUOTES | ENT_HTML5, 'UTF-8');
echo "decoded=$decoded:", bin2hex($decoded), "\n";
"#,
        ),
        concat!(
            "yes:&nbsp;:253\n",
            "updated:yes\n",
            "252\n",
            "decoded=Æ⫽⃥:c386e2abbde283a5\n",
        )
    );
}
