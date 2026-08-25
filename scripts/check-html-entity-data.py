#!/usr/bin/env python3
"""Generate or verify RPHP's HTML entity tables from public standards/PHP."""

import argparse
import html
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.request


HTML4_URL = "https://www.w3.org/TR/html4/sgml/entities.html"
HTML5_URL = "https://html.spec.whatwg.org/entities.json"
ROOT = Path(__file__).resolve().parent.parent


def fetch_text(url: str) -> str:
    with urllib.request.urlopen(url) as response:
        return response.read().decode()


def php_map(reference_php: str, flags: int) -> dict[str, str]:
    code = (
        "echo json_encode(get_html_translation_table(HTML_ENTITIES,"
        f"{flags},'UTF-8'),JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES);"
    )
    return json.loads(subprocess.check_output([reference_php, "-r", code]))


def rust_string(value: str) -> str:
    return "".join(f"\\u{{{ord(character):x}}}" for character in value)


def generated_source(reference_php: str) -> str:
    html5_standard = json.loads(fetch_text(HTML5_URL))
    html4_page = html.unescape(fetch_text(HTML4_URL))
    html4_standard = {
        chr(int(codepoint)): f"&{name};"
        for name, codepoint in re.findall(
            r'<!ENTITY\s+(\w+)\s+CDATA\s+"&#(\d+);"', html4_page
        )
    }
    html4 = php_map(reference_php, 3)
    html5 = php_map(reference_php, 3 | 48)
    html5_decode = {
        name.removeprefix("&"): item["characters"]
        for name, item in html5_standard.items()
        if name.endswith(";")
    }

    assert len(html4_standard) == 252
    expected_html4 = dict(html4_standard)
    expected_html4["'"] = "&#039;"
    assert html4 == expected_html4
    assert len(html4) == 253 and len(html5) == 1511
    assert len(html5_decode) == 2125
    assert list(html4) == sorted(html4)
    assert list(html5) == sorted(html5)
    for characters, entity in html5.items():
        assert html5_standard[entity]["characters"] == characters
    standard_values = {
        item["characters"]
        for name, item in html5_standard.items()
        if name.endswith(";")
    }
    assert set(html5) == standard_values

    lines = [
        "// Generated from the public W3C HTML4 entity catalog and WHATWG",
        "// entities.json. PHP 8.5 differential checks choose canonical aliases;",
        "// no php-src source or tests are used.",
        f"// {HTML4_URL}",
        f"// {HTML5_URL}",
    ]
    for name, mapping in (
        ("HTML4_ENTITIES", html4),
        ("HTML5_ENTITIES", html5),
    ):
        lines.append(f"pub(super) const {name}: &[(&str, &str)] = &[")
        lines.extend(
            f'    ("{rust_string(characters)}", "{entity}"),'
            for characters, entity in mapping.items()
        )
        lines.append("];")
    decode_data = bytearray()
    decode_offsets = []
    lines.append("const HTML5_DECODE_DATA: &str = concat!(")
    for name, characters in sorted(html5_decode.items()):
        encoded_name = name.encode()
        encoded_characters = characters.encode()
        assert len(encoded_name) <= 255 and len(encoded_characters) <= 255
        decode_offsets.append(len(decode_data))
        decode_data.extend((len(encoded_name), len(encoded_characters)))
        decode_data.extend(encoded_name)
        decode_data.extend(encoded_characters)
        lines.append(
            f'    "\\x{len(encoded_name):02x}\\x{len(encoded_characters):02x}'
            f'{name}{rust_string(characters)}",'
        )
    lines.append(");")
    assert len(decode_data) <= 65535
    assert max(decode_offsets) <= 65535
    lines.append("#[rustfmt::skip]")
    lines.append(
        f"const HTML5_DECODE_OFFSETS: [u16; {len(decode_offsets)}] = ["
    )
    lines.extend(f"    {offset}," for offset in decode_offsets)
    lines.extend(
        [
            "];",
            "pub(super) fn html5_characters_for_entity(name: &str) -> Option<&'static str> {",
            "    let data = HTML5_DECODE_DATA.as_bytes();",
            "    let position = HTML5_DECODE_OFFSETS",
            "        .binary_search_by(|offset| {",
            "            let start = usize::from(*offset);",
            "            let end = start + 2 + usize::from(data[start]);",
            "            data[start + 2..end].cmp(name.as_bytes())",
            "        })",
            "        .ok()?;",
            "    let start = usize::from(HTML5_DECODE_OFFSETS[position]);",
            "    let characters_start = start + 2 + usize::from(data[start]);",
            "    let characters_end = characters_start + usize::from(data[start + 1]);",
            "    std::str::from_utf8(&data[characters_start..characters_end]).ok()",
            "}",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--write", action="store_true", help="rewrite the checked-in generated source"
    )
    args = parser.parse_args()
    reference_php = os.environ.get("RPHP_REFERENCE_PHP", "php")
    version = subprocess.check_output(
        [reference_php, "-r", "echo PHP_MAJOR_VERSION,'.',PHP_MINOR_VERSION;"]
    ).decode()
    if version != "8.5":
        raise SystemExit(f"reference PHP must be 8.5, got {version}")
    path = ROOT / "src/stdlib/html_entities.rs"
    generated = generated_source(reference_php)
    if args.write:
        path.write_text(generated)
        print(f"wrote {path.relative_to(ROOT)} from W3C, WHATWG and PHP {version}")
    elif path.read_text() != generated:
        raise SystemExit(f"generated entity data is stale: {path}")
    else:
        print(
            f"verified {path.relative_to(ROOT)} against W3C, WHATWG and PHP {version}"
        )


if __name__ == "__main__":
    main()
