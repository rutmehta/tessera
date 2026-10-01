#!/usr/bin/env python3
"""B5-44 source enumeration; no GUI, third-party modules, or checked-in key manifest.

Walk the SwiftUI Commands' referenced View definitions, not every sheet's buttons.
Disabled items still register key equivalents; only conditional construction or
.shortcut(false, ...) removes one. Unknown shortcut expressions fail closed.
"""
import argparse
from collections import defaultdict
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / 'apps/mac/Sources'
DOC = ROOT / 'docs/shortcuts.md'
MODS = {'control': '⌃', 'option': '⌥', 'shift': '⇧', 'command': '⌘'}
RESERVED = {'⌘Q', '⌘H', '⌥⌘H', '⌘M', '⌘,', '⌘Tab', '⌃⌘Q', '⌘Space',
            '⇧⌘3', '⇧⌘4', '⇧⌘5', '⌃F2', '⌃F3', 'F11', 'F12', '⌘`'}
BOTH = frozenset({'Library', 'Document'})
TOKEN = re.compile(r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"')


def structural(text):
    return TOKEN.sub(lambda m: ''.join('\n' if c == '\n' else ' ' for c in m[0]), text)


def block(text, opening):
    masked = structural(text)
    depth = 1
    for end in range(opening + 1, len(text)):
        depth += (masked[end] == '{') - (masked[end] == '}')
        if depth == 0:
            return text[opening + 1:end], end
    raise ValueError('Unbalanced Swift block')


def scope(text, offset, inherited):
    """Only explicit mode construction gates remove registered key equivalents."""
    result = inherited
    for match in re.finditer(r'\bif\s+(!?docMode)\s*\{', structural(text)):
        _, end = block(text, match.end() - 1)
        if match.end() <= offset < end:
            result &= frozenset({'Library' if match[1].startswith('!') else 'Document'})
    return result


def chord(key, modifiers):
    names = re.findall(r'\.(\w+)', modifiers)
    if any(name not in MODS for name in names):
        raise ValueError('Unknown modifiers: ' + modifiers)
    key = key.strip('"')
    key = {'.delete': 'Delete', '.tab': 'Tab', '.space': 'Space', '.escape': 'Escape',
           '.return': 'Return', ' ': 'Space', '\\t': 'Tab'}.get(key, key)
    return ''.join(symbol for name, symbol in MODS.items() if name in names) + (key.upper() if len(key) == 1 else key)


def menu_bindings():
    definitions = {}
    for path in sorted((SRC / 'Tessera').rglob('*.swift')):
        text = path.read_text()
        for match in re.finditer(r'\bstruct\s+(\w+)\s*:\s*(?:View|Commands)\s*\{', structural(text)):
            body, _ = block(text, match.end() - 1)
            definitions[match[1]] = (path, body)
    result, visited = [], set()

    def walk(name, modes):
        if (name, modes) in visited:
            return
        visited.add((name, modes))
        path, body = definitions[name]
        # Strip comments but preserve strings for literal keys and command labels.
        body = re.sub(r'//[^\n]*|/\*[\s\S]*?\*/', '', body)
        for match in re.finditer(r'\.(keyboardShortcut|shortcut)\(', body):
            tail = body[match.end():]
            pattern = (r'\s*("[^"\n]*"|\.\w+)\s*,\s*modifiers:\s*(\[[^\]]*\]|\.\w+)\s*\)'
                       if match[1] == 'keyboardShortcut' else
                       r'\s*([^,]+),\s*("[^"\n]*"|\.\w+)\s*,\s*(\[[^\]]*\]|\.\w+)\s*\)')
            parsed = re.match(pattern, tail)
            if not parsed:
                raise ValueError(f'{name}: unsupported shortcut: {tail.splitlines()[0]}')
            active = scope(body, match.start(), modes)
            if match[1] == 'shortcut':
                condition, key, modifiers = parsed.groups()
                if condition.strip() not in ('docMode', '!docMode', 'doc != nil'):
                    raise ValueError('Unknown shortcut condition: ' + condition)
                active &= frozenset({'Library' if condition.strip() == '!docMode' else 'Document'})
            else:
                key, modifiers = parsed.groups()
            # For switch-generated buttons use the case, otherwise the nearest Button/Toggle.
            before = body[:match.start()]
            case = re.search(r'case\s+\.(\w+):\s*button$', before)
            labels = list(re.finditer(r'(?:Button|Toggle)\(([^\n]+)', before))
            label = case[1] if case else labels[-1][1].split(' {')[0].split(', isOn:')[0].rstrip(')')
            titles = re.findall(r'"([^"]+)"', label)
            label = ' / '.join(titles) if titles else label
            label = {'model.undoMenuTitle': 'Undo (dynamic title)',
                     'model.redoMenuTitle': 'Redo (dynamic title)',
                     'filters.lastFilterTitle': 'Last Filter (dynamic title)',
                     'DocumentCameraRaw.menuTitle': 'Camera Raw Filter…',
                     'AdaptiveWideAngleFilter.menuTitle': 'Adaptive Wide Angle…'}.get(label, label)
            result.append((active, chord(key, modifiers), name + ' / ' + label, str(path.relative_to(ROOT))))
        for call in re.finditer(r'\b([A-Z]\w*)\(', structural(body)):
            if call[1] in definitions:
                walk(call[1], scope(body, call.start(), modes))
    walk('AppCommands', BOTH)
    if len(result) < 50:
        raise ValueError('Incomplete menu traversal')
    return result


def route_sources():
    """Include the actual routing guards/priority as well as every case in the docs.

    The appendix makes a new conditional binding fail documentation verification too,
    even when it is not expressed as a switch case. Runtime map tests cover tool groups
    and reserved event dispatch without recreating production routing in this script.
    """
    paths = ['Tessera/App/KeyRouter.swift', 'TesseraCore/Document/DocumentKeyMap.swift',
             'TesseraCore/Document/Tools/EditorTools.swift', 'Tessera/App/MaskTools.swift',
             'Tessera/Document/Tools/DocumentTools.swift', 'Tessera/Document/Retouch/DocumentRetouch.swift',
             'Tessera/Document/Vector/DocumentVector.swift', 'Tessera/Document/Text/DocumentText.swift',
             'Tessera/Document/Transforms/DocumentTransforms.swift',
             'Tessera/Document/ContentAware/DocumentContentAware.swift']
    # Develop's handler lives in an extension; discover it rather than guess its file.
    paths += [str(p.relative_to(SRC)) for p in sorted((SRC / 'Tessera').rglob('*.swift'))
              if 'extension DevelopTools' in p.read_text() and 'func handleKey(' in p.read_text()]
    result = []
    for rel in paths:
        text = (SRC / rel).read_text()
        if rel.endswith('KeyRouter.swift'):
            text = re.sub(r'//[^\n]*', '', text)
            result.append((rel, '\n'.join(line.rstrip() for line in text.splitlines() if line.strip())))
            continue
        regions = []
        for match in re.finditer(r'(?:public |private |static )*func (?:action|handleKey|routeSessionKey|keyAction)\([^\{]+\{', structural(text)):
            body, end = block(text, match.end() - 1)
            regions.append(text[match.start():end + 1])
        if rel.endswith('EditorTools.swift'):
            for prop in ('key', 'group'):
                match = re.search(r'public var ' + prop + r':[^\{]+\{', text)
                _, end = block(text, match.end() - 1)
                regions.insert(0, text[match.start():end + 1])
        if not regions:
            raise ValueError('No routing found: ' + rel)
        result.append((rel, '\n\n'.join(regions)))
    return result


# Hardware spellings are shared with NSEvent, not copied command assignments.
KEY_CODES = {36: 'Return', 76: 'Enter', 53: 'Escape', 51: 'Delete', 117: 'ForwardDelete',
             123: 'Left', 124: 'Right', 125: 'Down', 126: 'Up', 49: 'Space', 48: 'Tab'}


def routed_bindings():
    text = (SRC / 'Tessera/App/KeyRouter.swift').read_text()
    rows = []
    for name, context in [('handle', 'Library/Cull'), ('handleReview', 'Library/Review'),
                          ('handlePhotoEdit', 'Library/Photo Edit')]:
        match = re.search(r'func ' + name + r'\([^\n]+\{', text)
        body, _ = block(text, match.end() - 1)
        body = re.sub(r'//[^\n]*', '', body)
        for case in re.finditer(r'case ([^:\n]+):([^\n]*)', body):
            literal = case[1]
            if not re.match(r'["0-9]', literal):
                continue
            keys = re.findall(r'"([^"\n]+)"|\b(\d+)\b', literal.split(' where ')[0])
            for char, code in keys:
                key = char.upper() if char else KEY_CODES[int(code)]
                action = case[2].strip() or 'context-dependent action (see routing reference)'
                row_context = context
                # Compare's first switch precedes the general culling switch.
                if name == 'handle' and case.start() < body.index('switch event.keyCode', body.index('if comparing {') + len('if comparing {')):
                    raise ValueError('Unexpected switch ordering')
                if name == 'handle' and case.start() < body.index('switch event.keyCode', body.index('default: break') + 1):
                    row_context = 'Library/Compare override'
                if 'where comparing' in literal:
                    row_context = 'Library/Compare override'
                if 'where loupe' in literal:
                    row_context = 'Library/Loupe'
                rows.append((row_context, key, action))
    # Tool key groups are derived from the real key property; one palette group is one binding.
    tools = (SRC / 'TesseraCore/Document/Tools/EditorTools.swift').read_text()
    match = re.search(r'public var key: String \{', tools)
    body, _ = block(tools, match.end() - 1)
    for case in re.finditer(r'case ([^:]+): "([^"]+)"', body):
        rows.append(('Document/Tools', case[2], case[1].strip()))
    # Include the modifiers accepted by KeyRouter's unmodified-key branch. These
    # are aliases of one action, not competing handlers. Cmd/Control pass to menus.
    library = [row for row in rows if row[0].startswith('Library/')]
    for context, key, action in library:
        for prefix in ('⇧', '⌥', '⌥⇧'):
            rows.append((context, prefix + key, action))
    # The tool map accepts Shift for every group, except Shift-J, which is owned
    # by DocumentRetouch before ToolKeyMap (documented priority, not two actions).
    for context, key, action in list(rows):
        if context == 'Document/Tools':
            rows.append((context, '⇧' + key, 'Remove tool' if key == 'J' else action))
    # Extract the non-tool keys from the actual map's literal cases.
    start = tools.index('public enum ToolKeyMap')
    for case in re.finditer(r'case ([^:\n]+): return ([^\n]+)', tools[start:]):
        chars = re.findall(r'"([^"]+)"', case[1])
        codes = re.findall(r'\b(\d+)\b', case[1]) if not chars else []
        for key in chars + [KEY_CODES[int(c)] for c in codes]:
            # Braces are the shifted spelling of brackets, not a second binding.
            if key in ('{', '}'):
                continue
            rows.append(('Document/Tools', key.upper() if len(key) == 1 else key, case[2]))
            if '!shift' not in case[1]:
                rows.append(('Document/Tools', '⇧' + key, case[2]))
    if 'let d = Int(ch)' in tools[start:]:
        rows += [('Document/Tools', str(d), 'opacityDigit') for d in range(10)]
    # DocumentKeyMap is a fallback. V/M alias the same tools; its Command branch
    # never runs through KeyRouter. All remaining fallback cases are independent.
    fallback = (SRC / 'TesseraCore/Document/DocumentKeyMap.swift').read_text()
    fallback = fallback[fallback.index('switch keyCode {'):]
    for case in re.finditer(r'case ([^:\n]+): return ([^\n]+)', fallback):
        if '.tool(' in case[2]:
            continue
        chars = re.findall(r'"([^"]+)"', case[1])
        codes = re.findall(r'\b(\d+)\b', case[1]) if not chars else []
        for key in chars + [KEY_CODES[int(c)] for c in codes]:
            key = key.upper() if len(key) == 1 else key
            rows.append(('Document/Tools', key, case[2]))
            if key in ('Space', 'F'):
                rows.append(('Document/Tools', '⇧' + key, case[2]))
            if key == 'Space':
                rows += [('Document/Tools', prefix + key, case[2]) for prefix in ('⌥', '⌥⇧')]
    quick_mask = re.search(r'charactersIgnoringModifiers\?\.lowercased\(\) == "([^"]+)"',
                           text[text.index('private func handleDocument'):])
    rows.append(('Document/Tools', quick_mask[1].upper(), 'Quick Mask'))
    edit = text[text.index('func handleEditInLayers'):text.index('func handleKeyUp')]
    key = re.search(r'charactersIgnoringModifiers\?\.lowercased\(\) == "([^"]+)"', edit)[1]
    if '== .command' not in edit or 'model.requestLayeredCopy()' not in edit:
        raise ValueError('Review the Edit in Layers menu/router alias')
    rows.append(('Library/Menu alias', '⌘' + key.upper(), 'Open in Layers'))
    return rows


def issues(bindings):
    errors, seen = [], defaultdict(list)
    for modes, key, label, _ in bindings:
        if key in RESERVED:
            errors.append(f'Reserved macOS shortcut {key}: {label}')
        for mode in sorted(modes):
            seen[mode, key].append(label)
    for (mode, key), labels in sorted(seen.items()):
        if len(labels) > 1:
            errors.append(f'Duplicate {mode} {key}: ' + ' <> '.join(labels))
    routes = routed_bindings()
    routed_seen = defaultdict(list)
    for context, key, action in routes:
        routed_seen[context, key].append(action)
        mode = context.split('/')[0]
        if key in RESERVED:
            errors.append(f'Reserved macOS shortcut {key}: {context} {action}')
        for modes, menu_key, label, _ in bindings:
            if mode in modes and menu_key == key:
                if context == 'Library/Menu alias' and label == 'AppCommands / Open in Layers…':
                    continue  # Same model.requestLayeredCopy action, intentionally routed locally.
                errors.append(f'Duplicate {context} {key}: router {action} <> {label}')
    for (context, key), actions in routed_seen.items():
        if len(actions) > 1:
            errors.append(f'Duplicate {context} {key}: ' + ' <> '.join(actions))
    return errors


def render(bindings):
    lines = ['# Tessera keyboard shortcuts', '',
             'Generated from SwiftUI Commands, KeyRouter and its delegated key maps. Regenerate with',
             '`python3 tools/orchestrate/shortcut-audit.py --write-doc`; the Swift gate checks this file.', '',
             '## Menu key equivalents', '',
             'Modes describe registered equivalents, including disabled items and diagnostic commands.',
             'A disabled command may be unavailable for the current selection. Conditional `.shortcut`',
             'bindings are absent outside their mode. System-provided Quit, Hide, Minimize, Settings,',
             'window cycling and other standard macOS commands retain their native meanings; custom',
             'commands may not claim the reserved combinations listed below.', '',
             '| Mode | Shortcut | Command |', '| --- | --- | --- |']
    for modes, key, label, _ in bindings:
        lines.append('| ' + ' / '.join(sorted(modes)) + ' | ' + key + ' | ' + label.replace('|', '\\|') + ' |')
    lines += ['', '## Reserved for macOS', '', ', '.join(sorted(RESERVED)), '',
              '## Routed keys and context', '',
              'Library culling, Compare, Review and Photo Edit have separate routing contexts.',
              'Document tools run before the fallback document map. Tool groups intentionally share',
              'a letter; Shift cycles that group (A also cycles on repetition). Shift-J activates Remove',
              'before the Healing tool map. Return/Escape/Delete act on the active edit, selection or',
              'tool session before falling back. Command-E in Library is a router alias of Open in Layers.',
              'Command entries in DocumentKeyMap describe menu actions; KeyRouter passes these to menus.',
              'Text fields, key-owning controls, panels and sheets have the priority shown in the guards.', '',
              'The following source-generated reference includes every routed key, its action, modifiers',
              'and precedence, including consumed keys that deliberately do nothing in Review/Photo Edit.',
              'Keeping the guards here avoids documenting context-dependent keys as global shortcuts.', '']
    lines += ['| Context | Key | Action / tool group |', '| --- | --- | --- |']
    for context, key, action in routed_bindings():
        lines.append(f'| {context} | {key} | {action.replace("|", "&#124;")} |')
    lines += ['', 'Library letters also accept Shift/Option; Shift-S selects gamut warning, and',
              'Shift/Option arrows extend selection or navigate groups. The full modifier guards,',
              'special keys, digit/bracket maps and delegated edit-session bindings follow.', '']
    for path, source in route_sources():
        lines += ['### ' + path, '', '```swift', source, '```', '']
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--write-doc', action='store_true')
    args = parser.parse_args()
    bindings = menu_bindings()
    errors = issues(bindings)
    expected = render(bindings)
    if args.write_doc:
        if errors:
            print('\n'.join(errors))
            return 1
        DOC.write_text(expected)
    elif not DOC.exists() or DOC.read_text() != expected:
        errors.append('Undocumented or stale shortcuts: regenerate docs/shortcuts.md from the enumeration')
    print(f'Enumerated {len(bindings)} menu bindings; {len(route_sources())} routing sources; {len(RESERVED)} reserved chords')
    print('\n'.join(errors) if errors else 'SHORTCUT AUDIT OK')
    return bool(errors)


if __name__ == '__main__':
    sys.exit(main())
