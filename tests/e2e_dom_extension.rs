mod common;

use common::run_php;

#[test]
fn dom_exports_php_85_extension_and_legacy_class_hierarchy() {
    assert_eq!(
        run_php(
            r#"<?php
echo (int) extension_loaded('dom'), (int) extension_loaded('DOM'), "\n";
echo implode(',', get_extension_funcs('dom')), "\n";
foreach (['DOMDocument', 'DOMElement', 'DOMXPath', 'DOMNodeList', 'DOMException'] as $name) {
    $class = new ReflectionClass($name);
    $parent = $class->getParentClass();
    echo $name, ':', $class->getExtensionName(), ':',
        $parent ? $parent->getName() : '-', ':',
        implode(',', $class->getInterfaceNames()), "\n";
}
foreach ([
    ['DOMDocument', 'loadXML'], ['DOMDocument', 'saveXML'],
    ['DOMElement', 'setAttribute'], ['DOMXPath', 'query'],
    ['DOMNodeList', 'item'], ['DOMXPath', 'quote'],
    ['DOMNodeList', 'getIterator']
] as [$class, $method]) {
    $reflection = new ReflectionMethod($class, $method);
    echo $class, '::', $method, ':',
        $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':',
        $reflection->getReturnType() ?: '-', "\n";
}
"#,
        ),
        concat!(
            "11\n",
            "dom_import_simplexml,Dom\\import_simplexml\n",
            "DOMDocument:dom:DOMNode:DOMParentNode\n",
            "DOMElement:dom:DOMNode:DOMParentNode,DOMChildNode\n",
            "DOMXPath:dom:-:\n",
            "DOMNodeList:dom:-:IteratorAggregate,Traversable,Countable\n",
            "DOMException:dom:Exception:Throwable,Stringable\n",
            "DOMDocument::loadXML:1/2:-\n",
            "DOMDocument::saveXML:0/2:-\n",
            "DOMElement::setAttribute:2/2:-\n",
            "DOMXPath::query:1/3:-\n",
            "DOMNodeList::item:1/1:-\n",
            "DOMXPath::quote:1/1:string\n",
            "DOMNodeList::getIterator:0/0:Iterator\n",
        )
    );
}

#[test]
fn dom_phpunit_configuration_traversal_and_comment_removal_are_exact() {
    assert_eq!(
        run_php(
            r#"<?php
$document = new DOMDocument('1.0', 'UTF-8');
$document->preserveWhiteSpace = false;
var_dump($document->loadXML('<phpunit><testsuites><testsuite name="unit"/><testsuite name="integration"/></testsuites><!-- remove --></phpunit>'));
$xpath = new DOMXPath($document);
$testsuites = $xpath->query('testsuites/testsuite', $document->documentElement);
var_dump($testsuites->length, count($testsuites), $testsuites->item(1)->getAttribute('name'));
foreach ($xpath->query('//comment()') as $comment) {
    $comment->parentNode->removeChild($comment);
}
$document->documentElement->append($document->createElement('tail', 'ok'));
echo $document->saveXML();
"#,
        ),
        concat!(
            "bool(true)\n",
            "int(2)\n",
            "int(2)\n",
            "string(11) \"integration\"\n",
            "<?xml version=\"1.0\"?>\n",
            "<phpunit><testsuites><testsuite name=\"unit\"/>",
            "<testsuite name=\"integration\"/></testsuites><tail>ok</tail></phpunit>\n",
        )
    );
}

#[test]
fn dom_mutation_pretty_serialization_and_escaping_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
$document = new DOMDocument();
$document->preserveWhiteSpace = false;
$document->formatOutput = true;
$document->loadXML('<r>  <a id="1">x&amp;y</a><a id="2"><b/></a></r>');
$document->documentElement->setAttribute('z', '<&"');
echo $document->saveXML();
"#,
        ),
        concat!(
            "<?xml version=\"1.0\"?>\n",
            "<r z=\"&lt;&amp;&quot;\">\n",
            "  <a id=\"1\">x&amp;y</a>\n",
            "  <a id=\"2\">\n",
            "    <b/>\n",
            "  </a>\n",
            "</r>\n",
        )
    );
}

#[test]
fn dom_namespaces_collections_and_canonical_output_share_one_tree() {
    assert_eq!(
        run_php(
            r#"<?php
$document = new DOMDocument();
$root = $document->createElementNS('urn:root', 'p:root');
$document->appendChild($root);
$child = $document->createElementNS('urn:child', 'q:child', 'value');
$child->setAttributeNS('urn:attr', 'a:id', '7');
$root->appendChild($child);
var_dump($document->getElementsByTagNameNS('urn:child', 'child')->length);
var_dump($child->hasAttributeNS('urn:attr', 'id'));
var_dump($root->C14N());
"#,
        ),
        concat!(
            "int(1)\n",
            "bool(true)\n",
            "string(108) \"<p:root xmlns:p=\"urn:root\"><q:child xmlns:a=\"urn:attr\" ",
            "xmlns:q=\"urn:child\" a:id=\"7\">value</q:child></p:root>\"\n",
        )
    );
}

#[test]
fn dom_parse_failures_follow_request_local_libxml_error_policy() {
    assert_eq!(
        run_php(
            r#"<?php
libxml_use_internal_errors(true);
$document = new DOMDocument();
var_dump($document->loadXML('<root><child></root>'));
$errors = libxml_get_errors();
var_dump(count($errors), $errors[0] instanceof LibXMLError, $errors[0]->line > 0);
libxml_clear_errors();
var_dump(libxml_get_errors());
"#,
        ),
        concat!(
            "bool(false)\n",
            "int(1)\n",
            "bool(true)\n",
            "bool(true)\n",
            "array(0) {\n}\n",
        )
    );
}

#[test]
fn dom_node_and_exception_constants_match_php_85() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    'XML_ELEMENT_NODE', 'XML_ATTRIBUTE_NODE', 'XML_TEXT_NODE',
    'XML_DOCUMENT_NODE', 'XML_NAMESPACE_DECL_NODE', 'XML_LOCAL_NAMESPACE',
    'XML_ATTRIBUTE_ID', 'XML_ATTRIBUTE_NOTATION', 'DOM_PHP_ERR',
    'DOM_HIERARCHY_REQUEST_ERR', 'DOM_NAMESPACE_ERR', 'DOM_VALIDATION_ERR'
] as $name) {
    echo $name, '=', constant($name), "\n";
}
echo DOMNode::DOCUMENT_POSITION_DISCONNECTED, ',',
    DOMNode::DOCUMENT_POSITION_CONTAINED_BY, ',',
    DOMNode::DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC, "\n";
"#,
        ),
        concat!(
            "XML_ELEMENT_NODE=1\n",
            "XML_ATTRIBUTE_NODE=2\n",
            "XML_TEXT_NODE=3\n",
            "XML_DOCUMENT_NODE=9\n",
            "XML_NAMESPACE_DECL_NODE=18\n",
            "XML_LOCAL_NAMESPACE=18\n",
            "XML_ATTRIBUTE_ID=2\n",
            "XML_ATTRIBUTE_NOTATION=10\n",
            "DOM_PHP_ERR=0\n",
            "DOM_HIERARCHY_REQUEST_ERR=3\n",
            "DOM_NAMESPACE_ERR=14\n",
            "DOM_VALIDATION_ERR=16\n",
            "1,16,32\n",
        )
    );
}

#[test]
fn dom_schema_validation_parses_xsd_and_checks_the_global_document_root() {
    assert_eq!(
        run_php(
            r#"<?php
libxml_use_internal_errors(true);
$document = new DOMDocument();
$document->loadXML('<phpunit/>');
$valid = <<<'XSD'
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="phpunit"/>
</xs:schema>
XSD;
$invalid = <<<'XSD'
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="different"/>
</xs:schema>
XSD;
var_dump($document->schemaValidateSource($valid));
var_dump($document->schemaValidateSource($invalid));
$errors = libxml_get_errors();
var_dump(count($errors), str_contains($errors[0]->message, 'No matching global declaration'));
try {
    $document->schemaValidateSource('');
} catch (ValueError $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(false)\n",
            "int(1)\n",
            "bool(true)\n",
            "DOMDocument::schemaValidateSource(): Argument #1 ($source) must not be empty\n",
        )
    );
}
