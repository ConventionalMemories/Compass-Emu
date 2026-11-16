"""Make CHANGELOG.md and docs/user-guide.md from dist/windows/README.txt (the text that
ships in the Windows zip). CHANGELOG.md's "## Unreleased" section is hand-written and kept. Run from the repository root after editing README.txt."""
import re

lines = open('dist/windows/README.txt').read().split('\n')
i = next(k for k, l in enumerate(lines) if l.startswith('New in'))
guide, notes = lines[2:i], lines[i:]

out = ['# Changelog', '', "Compass-Emu versions (newest first). MartyPC's own changelog is in "
       '[MARTYPC-CHANGELOG.md](MARTYPC-CHANGELOG.md).', '']
for l in notes:
    m = re.match(r'^New in (\S+)', l)
    if m:
        out += ['## ' + m.group(1), '']
    elif l.startswith('Not there yet'):
        out += ['## Not there yet', '']
    else:
        out.append(l[2:] if l.startswith('  ') else l)
# the Unreleased section is written by hand in CHANGELOG.md: keep it
old = open('CHANGELOG.md').read()
m = re.search(r'^## Unreleased\n.*?(?=^## )', old, re.S | re.M)
if m and '## Unreleased' not in out:
    out[4:4] = m.group(0).rstrip('\n').split('\n') + ['']
open('CHANGELOG.md', 'w').write('\n'.join(out).rstrip() + '\n')

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
