"""Make docs/user-guide.md from dist/windows/README.txt (the text that ships in the Windows
zip). Run from the repository root after editing README.txt. CHANGELOG.md is edited by hand.

README.txt layout (plain text that reads well in Notepad):
- A line starting in column 0 is a section title. Everything up to "Versions" is the guide.
- Section text is indented 2 spaces. Lines next to each other form one paragraph.
- "- " starts a bullet, "1. " a numbered step (a blank line ends the list); the item's own text goes on at the column
  where its first word starts. A "- " at that column inside an item is a nested bullet.
- Lines indented 4 or more spaces past the current text column are an example (code block).
- In the "Command line" section, an option line is "  --name args  text"; its text goes on
  at column 13. Lines indented 6 or more inside it are examples.
"""
import re

SRC = 'dist/windows/README.txt'
DST = 'docs/user-guide.md'


def indent(l):
    return len(l) - len(l.lstrip(' '))


ITEM = re.compile(r'^(- |\d+\. )')


def prose(body):
    """Paragraphs, (nested) lists and code blocks -> Markdown."""
    out = []
    items = []      # open list items: (marker column, text column, markdown indent of content)
    para = []       # current paragraph lines, first one with its markdown prefix
    code = []
    base_col = [0]  # column the current example starts at

    def text_col():
        return items[-1][1] if items else 2

    def md():
        return items[-1][2] if items else ''

    def flush_para():
        if para:
            out.append(' '.join(para))
            para.clear()

    def flush_code():
        while code and code[-1] == '':
            code.pop()
        if code:
            ind = md()
            out.append(ind + '```')
            out.extend((ind + c).rstrip() for c in code)
            out.append(ind + '```')
            code.clear()

    for raw in body:
        if not raw.strip():
            if code:
                code.append('')
            else:
                flush_para()
                items.clear()               # a blank line ends any list
                if out and out[-1] != '':
                    out.append('')
            continue
        col = indent(raw)
        text = raw.strip()
        if col < text_col() + 4:
            text = text.replace('*', '\\*')  # no emphasis in prose
        if col >= text_col() + 4:           # an example
            if not code:
                flush_para()
                base_col[0] = col
            code.append(raw[min(col, base_col[0]):].rstrip())
            continue
        flush_code()
        m = ITEM.match(text)
        # close the items this line is outside of (a sibling item closes its predecessor)
        closed = False
        while items and (col < items[-1][1]) and not (m and col >= items[-1][0]):
            flush_para()
            items.pop()
            closed = True
        sibling = bool(m and items and col == items[-1][0])
        if sibling:
            flush_para()
            items.pop()                     # the next item of the same list
        if closed and not m and out and out[-1] != '':
            out.append('')                  # a paragraph after a list needs a blank line
        if m:
            flush_para()
            if not items and not sibling and out and out[-1] != '':
                out.append('')              # a list needs a blank line before it
            marker = m.group(1)
            base = md()
            items.append((col, col + len(marker), base + ' ' * len(marker)))
            para.append(base + marker + text[len(marker):])
            continue
        if para:
            para.append(text)
        else:
            para.append(md() + ('\\' + text if text[0] in '+#>' else text))
    flush_para()
    flush_code()
    s = '\n'.join(out).strip('\n')
    return re.sub(r'\n{3,}', '\n\n', s) + '\n'


def command_line(body):
    """The option list -> a usage block and a table."""
    usage, rows, tail = [], [], []
    i = 0
    while i < len(body) and not body[i].lstrip().startswith('--'):
        if body[i].strip():
            usage.append(body[i][2:])
        i += 1
    for l in body[i:]:
        s = l.strip()
        if not s:
            continue
        if indent(l) == 2 and s.startswith(('--', 'compass')):
            m = re.match(r'(\S.*?)\s{2,}(.*)$', s)
            opt, text = (m.group(1), m.group(2)) if m else (s, '')
            rows.append([opt, text])
        elif indent(l) == 2:
            tail.append(s)
        elif rows and indent(l) <= 13:
            rows[-1][1] += ' ' + s
        elif rows:
            rows[-1][1] += ' `' + s + '`'
    g = '```\n' + '\n'.join(usage).strip('\n') + '\n```\n\n| Option | What it does |\n|---|---|\n'
    for opt, text in rows:
        g += '| `' + opt.replace('|', '\\|') + '` | ' + text.replace('|', '\\|') + ' |\n'
    if tail:
        g += '\n' + ' '.join(tail) + '\n'
    return g


def main():
    lines = open(SRC, encoding='utf-8').read().split('\n')
    end = next(k for k, l in enumerate(lines) if l.startswith('Versions'))
    guide = lines[2:end]
    g = '# Compass-Emu user guide\n\nThe same text ships as README.txt in the Windows zip.\n'
    sec, body = None, []

    def flush():
        nonlocal g
        if sec is None:
            return
        g += '\n## ' + sec + '\n\n'
        g += command_line(body) if sec == 'Command line' else prose(body)

    for l in guide:
        if l and not l.startswith(' '):
            flush()
            sec, body = l.strip(), []
        else:
            body.append(l)
    flush()
    open(DST, 'w', encoding='utf-8').write(g)


if __name__ == '__main__':
    main()
