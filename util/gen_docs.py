"""Make docs/user-guide.md from dist/windows/README.txt (the text that ships in the Windows
zip). Run from the repository root after editing README.txt. CHANGELOG.md is edited by hand."""
import re

lines = open('dist/windows/README.txt').read().split('\n')
i = next(k for k, l in enumerate(lines) if l.startswith('Versions'))
guide = lines[2:i]

g = '# Compass-Emu user guide\n\nThe same text ships as README.txt in the Windows zip.\n'
sec, body = None, []

def flush():
    global g
    if sec is None:
        return
    g += '\n## ' + sec + '\n\n'
    if 'Command line' in sec:
        g += '```\n' + '\n'.join(x[2:] if x.startswith('  ') else x for x in body).strip('\n') + '\n```\n'
        return
    # prose: plain lines; lines indented 4+ (examples) go in code blocks
    block = []
    text = []
    for x in body:
        if x.startswith('      '):
            block.append(x[6:])
            continue
        if block:
            text.append('```\n' + '\n'.join(block) + '\n```')
            block = []
        text.append(x.strip())
    if block:
        text.append('```\n' + '\n'.join(block) + '\n```')
    g += re.sub(r'\n{3,}', '\n\n', '\n'.join(text).strip('\n')) + '\n'

for l in guide:
    if l and not l.startswith(' '):
        flush()
        sec, body = l.strip(), []
    else:
        body.append(l)
flush()
open('docs/user-guide.md', 'w').write(g)
