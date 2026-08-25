#!/usr/bin/env python3
"""Verify RPHP's generated HTML entity tables against public standards/PHP."""

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

    assert len(html4_standard) == 252
    expected_html4 = dict(html4_standard)
    expected_html4["'"] = "&#039;"
    assert html4 == expected_html4
    assert len(html4) == 253 and len(html5) == 1511
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
    return "\n".join(lines) + "\n"


def main() -> None:
    reference_php = os.environ.get("RPHP_REFERENCE_PHP", "php")
    version = subprocess.check_output(
        [reference_php, "-r", "echo PHP_MAJOR_VERSION,'.',PHP_MINOR_VERSION;"]
    ).decode()
    if version != "8.5":
        raise SystemExit(f"reference PHP must be 8.5, got {version}")
    path = ROOT / "src/stdlib/html_entities.rs"
    if path.read_text() != generated_source(reference_php):
        raise SystemExit(f"generated entity data is stale: {path}")
    print(f"verified {path.relative_to(ROOT)} against W3C, WHATWG and PHP {version}")


if __name__ == "__main__":
    main()
