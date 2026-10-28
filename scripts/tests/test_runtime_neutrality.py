"""Production string/seed boundary; labelled ASMA test data and aliases have separate fences."""
import json
from pathlib import Path
import re
import unittest
ROOT = Path(__file__).resolve().parents[2]
FORBIDDEN = re.compile('(?i)\\bigor\\b|\\befrem\\b|carasent|asma[-_]\\d+')
raw_pattern = re.compile('(?:b|c)?r(#{0,255})"')
char_pattern = re.compile("'(?:\\\\(?:.|u\\{[0-9a-fA-F_]+\\})|[^'\\\\\\n])'")

def literals(text):
    i = 0
    while i < len(text):
        if text.startswith('//', i):
            end = text.find('\n', i)
            i = len(text) if end < 0 else end + 1
            continue
        if text.startswith('/*', i):
            depth = 1
            i += 2
            while i < len(text) and depth:
                if text.startswith('/*', i):
                    depth += 1
                    i += 2
                elif text.startswith('*/', i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            continue
        if text[i] == "'":
            char = char_pattern.match(text, i)
            if char:
                i += len(char.group())
                continue
        raw = raw_pattern.match(text, i) if text[i] in 'rbc' else None
        if raw:
            end = text.find('"' + raw.group(1), i + len(raw.group()))
            if end < 0:
                raise ValueError('unterminated raw string')
            end += 1 + len(raw.group(1))
            yield (i, text[i:end])
            i = end
            continue
        if text[i] == '"':
            start = i
            i += 1
            while i < len(text):
                if text[i] == '\\':
                    i += 2
                elif text[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
            yield (start, text[start:i])
            continue
        i += 1

class RuntimeNeutralityTests(unittest.TestCase):

    def test_production_strings_do_not_name_people_or_project_tickets(self):
        found = []
        for path in sorted((ROOT / 'crates').glob('*/src/**/*.rs')):
            if 'tests' in path.stem or 'tests' in path.parts:
                continue
            text = path.read_text()
            test_start = text.find('#[cfg(test)]')
            for start, value in literals(text):
                if test_start >= 0 and start > test_start:
                    continue
                if FORBIDDEN.search(value):
                    found.append(f'{path.relative_to(ROOT)}:{text.count(chr(10), 0, start) + 1}')
        self.assertEqual(found, [], 'production strings require neutral wording')

    def test_served_api_descriptions_are_neutral(self):
        document = json.loads((ROOT / 'crates/kontor-api/contract/openapi.json').read_text())
        def descriptions(value):
            if isinstance(value, dict):
                for key, child in value.items():
                    if key == 'description' and isinstance(child, str):
                        yield child
                    else:
                        yield from descriptions(child)
            elif isinstance(value, list):
                for child in value:
                    yield from descriptions(child)
        self.assertEqual([text for text in descriptions(document) if FORBIDDEN.search(text)], [])

    def test_seed_and_example_data_do_not_name_people_or_project_tickets(self):
        found = []
        for directory in ('config/examples', 'crates/kontor-profiles/fixtures', 'crates/kontor-teams/fixtures'):
            for path in sorted((ROOT / directory).rglob('*')):
                if path.is_file() and FORBIDDEN.search(path.read_text()):
                    found.append(str(path.relative_to(ROOT)))
        self.assertEqual(found, [])
if __name__ == '__main__':
    unittest.main()
