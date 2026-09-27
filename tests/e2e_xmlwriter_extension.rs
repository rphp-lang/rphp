mod common;

use common::run_php;

#[test]
fn xmlwriter_exports_php_85_extension_class_and_function_inventory() {
    assert_eq!(
        run_php(
            r#"<?php
echo (int) extension_loaded('xmlwriter'), (int) extension_loaded('XMLWriter'), "\n";
echo implode(',', get_extension_funcs('xmlwriter')), "\n";
$class = new ReflectionClass(XMLWriter::class);
echo $class->getExtensionName(), ':', (int) $class->isFinal(), ':',
    (int) $class->isCloneable(), ':', count($class->getMethods()), "\n";
foreach (['openMemory','writeElement','flush','toMemory','toStream'] as $name) {
    $method = $class->getMethod($name);
    echo $name, ':', (int) $method->isStatic(), ':',
        $method->getNumberOfRequiredParameters(), '/', $method->getNumberOfParameters(), ':',
        $method->getReturnType() ?: '-', "\n";
}
"#,
        ),
        concat!(
            "11\n",
            "xmlwriter_open_uri,xmlwriter_open_memory,xmlwriter_set_indent,",
            "xmlwriter_set_indent_string,xmlwriter_start_comment,xmlwriter_end_comment,",
            "xmlwriter_start_attribute,xmlwriter_end_attribute,xmlwriter_write_attribute,",
            "xmlwriter_start_attribute_ns,xmlwriter_write_attribute_ns,",
            "xmlwriter_start_element,xmlwriter_end_element,xmlwriter_full_end_element,",
            "xmlwriter_start_element_ns,xmlwriter_write_element,xmlwriter_write_element_ns,",
            "xmlwriter_start_pi,xmlwriter_end_pi,xmlwriter_write_pi,",
            "xmlwriter_start_cdata,xmlwriter_end_cdata,xmlwriter_write_cdata,",
            "xmlwriter_text,xmlwriter_write_raw,xmlwriter_start_document,",
            "xmlwriter_end_document,xmlwriter_write_comment,xmlwriter_start_dtd,",
            "xmlwriter_end_dtd,xmlwriter_write_dtd,xmlwriter_start_dtd_element,",
            "xmlwriter_end_dtd_element,xmlwriter_write_dtd_element,",
            "xmlwriter_start_dtd_attlist,xmlwriter_end_dtd_attlist,",
            "xmlwriter_write_dtd_attlist,xmlwriter_start_dtd_entity,",
            "xmlwriter_end_dtd_entity,xmlwriter_write_dtd_entity,",
            "xmlwriter_output_memory,xmlwriter_flush\n",
            "xmlwriter:0:0:45\n",
            "openMemory:0:0/0:-\n",
            "writeElement:0:1/2:-\n",
            "flush:0:0/1:-\n",
            "toMemory:1:0/0:static\n",
            "toStream:1:1/1:static\n",
        )
    );
}

#[test]
fn xmlwriter_memory_document_indentation_and_escaping_are_byte_exact() {
    assert_eq!(
        run_php(
            r#"<?php
$writer = XMLWriter::toMemory();
$writer->startDocument();
$writer->setIndent(true);
$writer->setIndentString('  ');
$writer->startElement('root');
$writer->writeAttribute('a', '<&"');
$writer->writeElement('child', 'x<&>');
$writer->writeElement('empty');
$writer->endElement();
$writer->endDocument();
echo $writer->outputMemory();
"#,
        ),
        concat!(
            "<?xml version=\"1.0\"?>\n",
            "<root a=\"&lt;&amp;&quot;\">\n",
            "  <child>x&lt;&amp;&gt;</child>\n",
            "  <empty/>\n",
            "</root>\n",
        )
    );
}

#[test]
fn xmlwriter_namespaces_cdata_comments_and_processing_instructions_match_php() {
    assert_eq!(
        run_php(
            r#"<?php
$writer = XMLWriter::toMemory();
$writer->startElementNs('p', 'root', 'urn:x');
$writer->writeAttributeNs('q', 'a', 'urn:q', '<&"');
$writer->writeCdata('a<&');
$writer->writeComment('c');
$writer->writePi('php', 'x=1');
$writer->endElement();
echo $writer->outputMemory();
"#,
        ),
        "<p:root q:a=\"&lt;&amp;&quot;\" xmlns:q=\"urn:q\" xmlns:p=\"urn:x\"><![CDATA[a<&]]><!--c--><?php x=1?></p:root>"
    );
}

#[test]
fn xmlwriter_memory_peek_and_flush_have_php_buffer_lifetime() {
    assert_eq!(
        run_php(
            r#"<?php
$writer = XMLWriter::toMemory();
$writer->startElement('r');
$writer->text('a');
var_dump($writer->outputMemory(false));
var_dump($writer->outputMemory(false));
var_dump($writer->outputMemory());
var_dump($writer->outputMemory());
"#,
        ),
        concat!(
            "string(4) \"<r>a\"\n",
            "string(4) \"<r>a\"\n",
            "string(4) \"<r>a\"\n",
            "string(0) \"\"\n",
        )
    );
}

#[test]
fn xmlwriter_dtd_one_shot_and_streaming_forms_are_equivalent() {
    assert_eq!(
        run_php(
            r#"<?php
$writer = XMLWriter::toMemory();
var_dump($writer->writeDtd('root', null, null, '<!ELEMENT root (#PCDATA)>'));
var_dump($writer->startDtd('root'));
var_dump($writer->writeDtdElement('root', '(#PCDATA)'));
var_dump($writer->endDtd());
var_dump($writer->outputMemory());
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "string(86) \"<!DOCTYPE root [<!ELEMENT root (#PCDATA)>]>",
            "<!DOCTYPE root [<!ELEMENT root (#PCDATA)>]>\"\n",
        )
    );
}

#[test]
fn xmlwriter_procedural_api_shares_the_object_state_machine() {
    assert_eq!(
        run_php(
            r#"<?php
$writer = xmlwriter_open_memory();
var_dump($writer instanceof XMLWriter);
xmlwriter_start_document($writer, '1.1', 'UTF-8', 'yes');
xmlwriter_start_element($writer, 'root');
xmlwriter_write_attribute($writer, 'id', '7');
xmlwriter_text($writer, '<ok>');
xmlwriter_full_end_element($writer);
xmlwriter_end_document($writer);
echo xmlwriter_output_memory($writer);
"#,
        ),
        concat!(
            "bool(true)\n",
            "<?xml version=\"1.1\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            "<root id=\"7\">&lt;ok&gt;</root>\n",
        )
    );
}

#[test]
fn xmlwriter_uri_and_stream_targets_flush_through_php_io() {
    assert_eq!(
        run_php(
            r#"<?php
$path = sys_get_temp_dir() . '/rphp-xmlwriter-e2e.xml';
@unlink($path);
$uri = XMLWriter::toUri($path);
$uri->writeElement('uri', 'ok');
var_dump($uri->flush());
echo file_get_contents($path), "\n";
@unlink($path);
$stream = fopen('php://output', 'w');
$writer = XMLWriter::toStream($stream);
$writer->writeElement('stream', 'ok');
var_dump($writer->flush());
"#,
        ),
        concat!(
            "int(13)\n",
            "<uri>ok</uri>\n",
            "<stream>ok</stream>int(19)\n",
        )
    );
}

#[test]
fn xmlwriter_rejects_uninitialized_and_clone_operations() {
    assert_eq!(
        run_php(
            r#"<?php
$writer = new XMLWriter();
try { $writer->outputMemory(); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { clone $writer; }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
try { XMLWriter::toStream(123); }
catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "Error:Invalid or uninitialized XMLWriter object\n",
            "Error:Trying to clone an uncloneable object of class XMLWriter\n",
            "TypeError:XMLWriter::toStream(): Argument #1 ($stream) must be a valid stream resource, int given\n",
        )
    );
}

#[test]
fn xmlwriter_static_factories_construct_the_late_called_class_before_initialization() {
    assert_eq!(
        run_php(
            r#"<?php
class ProjectWriter extends XMLWriter {
    public int $marker;
    public function __construct() {
        $this->marker = 17;
        echo "constructed\n";
    }
}
class BrokenWriter extends XMLWriter {
    public function __construct() { throw new Error('factory stopped'); }
}
$writer = ProjectWriter::toMemory();
echo get_class($writer), ':', $writer->marker, "\n";
$writer->writeElement('root');
echo $writer->outputMemory(), "\n";
try { BrokenWriter::toMemory(); }
catch (Throwable $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "constructed\n",
            "ProjectWriter:17\n",
            "<root/>\n",
            "factory stopped\n",
        )
    );
}

#[test]
fn xmlwriter_validates_names_and_rejects_a_closed_stream_resource() {
    assert_eq!(
        run_php(
            r#"<?php
$writer = XMLWriter::toMemory();
$writer->startElement('root');
foreach (['-1', '"'] as $name) {
    try { $writer->startAttribute($name); }
    catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
$stream = fopen('php://memory', 'w+');
fclose($stream);
try { XMLWriter::toStream($stream); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "XMLWriter::startAttribute(): Argument #2 ($name) must be a valid attribute name, \"-1\" given\n",
            "XMLWriter::startAttribute(): Argument #2 ($name) must be a valid attribute name, \"\"\" given\n",
            "XMLWriter::toStream(): supplied resource is not a valid stream resource\n",
        )
    );
}
