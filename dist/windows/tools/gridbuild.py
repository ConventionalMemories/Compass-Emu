#!/usr/bin/env python3
"""One-click builds for the GRiD Compass in MAME.

  gridbuild.py build SOURCE     compile, link and test-run SOURCE
  gridbuild.py debug SOURCE     the same, and keep the symbols for Compass-Emu's debugger
  gridbuild.py install SOURCE   put the last build of SOURCE on your disk

SOURCE is a file like projects\\hello\\hello.pas (or hello.run, or the folder), or a project name
("hello" means projects\\hello\\hello.pas).

build works on a temporary copy of the merged disk, so your own disk is never
changed (MAME may stay open). The old GRiD compiler and linker run on an
invisible GRiD: Compass-Emu (compass-emu\\compass.exe, a GRiD Compass 1129 by
default; GRID_MODEL=1139 etc. picks another model) or, if that is not there,
MAME (grid1139). Compass-Emu compiles on its fast CPU core (GRID_EXACT=1 uses the
cycle-exact one, much slower, same results). The results land next to the source:
  NAME.lst      compiler listing, with any errors
  NAME.run      the finished GRiD program (if it built)
  screen.png    the GRiD screen at the end
  output\\       files your program wrote on the GRiD
An empty file named norun in the project folder builds without starting the
program (for games that never quit).
debug builds the same program (the code is identical) with the compilers' DEBUG
option and also saves the object files (NAME.pas.obj, NAME.asm.obj) and the link
map (NAME.map) next to the source. In Compass-Emu, Debug > Load program symbols
and pick the project folder (or compass.exe --symbols projects\\NAME): procedure
names and source lines then show in the debugger, call stack, profiler and crash
reports, and breakpoints can be set by name.
install copies NAME.run and the source into the Projects subject of your
merged disk (made if it isn't there, with no password like the other subjects). MAME must be closed for that.
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
GRIDFS = os.path.join(HERE, 'gridfs.py')
LUA = os.path.join(HERE, 'gridbuild.lua')
DISK = os.environ.get('GRID_DISK', os.path.join(ROOT, 'disks', 'GRiD merged dev disk.img'))
MAME = os.environ.get('GRID_MAME', os.path.join(ROOT, 'grid.exe'))
EMU = os.environ.get('GRID_EMU', os.path.join(ROOT, 'compass-emu', 'compass.exe' if os.name == 'nt' else 'compass'))
# Compass-Emu model for builds: the 1129 (512K, 11xx build of GRiD-OS) by default.
MODEL = os.environ.get('GRID_MODEL', '1129')
SECONDS = os.environ.get('GRID_SECONDS', '10800')
ROMS = os.environ.get('GRID_ROMS', os.path.join(ROOT, 'roms'))
HD = "`'Hard Disk X'`"
INSTALL_SUBJECT = 'Projects'   # where builds run (on the temporary copy) and install puts programs

# Per language: the GRiD kind of the source file, the files the tools make,
# and the build script typed into the Development Executive.
LANGS = {
    '.pas': {
        'kind': '.pas~Text~',
        'outputs': ['{n}.pas~Obj~', '{n}.pas~Lst~', '{n}~Run~', '{n}~MP1~'],
        'listing': '{n}.pas~Lst~',
        'script': [
            'Prefix {hd}Projects',
            '{hd}Programs`Pascal {hd}Projects`{n}.pas~Text~',
            '{hd}Programs`Link {hd}Projects`{n}.pas~obj~, {hd}libs`CompactSystemCalls~lib~, '
            '{hd}libs`dqlarge~lnk~ To {hd}Projects`{n}~Run~ bind ss(stack(+1500)) purge fl pu',
            '{hd}Projects`{n}',
        ],
    },
}


# A Pascal program with more modules: every .asm file in the project folder
# is assembled, every other .pas file in it is compiled as a separate Pascal
# module (MODULE x; ... PRIVATE x; ...), and all are linked in (compact
# model). The .pas file named after the folder is the main program.
ASM_SCRIPT = [
    'Prefix {hd}Projects',
    '{asm}',
    '{pas}',
    '{hd}Programs`Pascal {hd}Projects`{n}.pas~Text~',
    '{hd}Programs`Link {hd}Projects`{n}.pas~obj~, {objs}{hd}libs`CompactCpCalls~lib~, '
    '{hd}libs`CompactSystemCalls~lib~, {hd}libs`8087~lib~, {hd}libs`dqlarge~lnk~ '
    'To {hd}Projects`{n}~Run~ bind ss(stack(+1500)) purge fl pu',
    '{hd}Projects`{n}',
]


def asm_modules(proj):
    mods = sorted(os.path.splitext(f)[0] for f in os.listdir(proj) if f.lower().endswith('.asm'))
    for m in mods:
        if not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]{0,30}', m):
            raise BuildError('Name %s.asm with letters, digits and _ only.' % m)
    return mods


def pas_modules(proj, main):
    mods = sorted(os.path.splitext(f)[0] for f in os.listdir(proj)
                  if f.lower().endswith('.pas') and os.path.splitext(f)[0].lower() != main.lower())
    for m in mods:
        if not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]{0,30}', m):
            raise BuildError('Name %s.pas with letters, digits and _ only.' % m)
    return mods


def save_screen(png, proj):
    # screen.png may be held open by a picture viewer or a sync program
    # (Windows then refuses to write it); never let that stop the build
    dst = os.path.join(proj, 'screen.png')
    try:
        shutil.copyfile(png, dst)
        return
    except OSError:
        pass
    try:
        os.remove(dst)
        shutil.copyfile(png, dst)
        return
    except OSError:
        pass
    for i in range(2, 100):
        alt = os.path.join(proj, 'screen%d.png' % i)
        try:
            shutil.copyfile(png, alt)
            print('screen.png could not be written (is it open somewhere?); the screen is in %s.' % alt)
            return
        except OSError:
            pass
    print('screen.png could not be written (is it open somewhere?).')


class BuildError(Exception):
    pass


def emu_state():
    # one saved state per model, merged disk contents and compass.exe (the temporary copy
    # can't be the key: gridfs stamps the files it writes with today's date)
    import hashlib
    h = hashlib.sha1()
    with open(DISK, 'rb') as f:
        h.update(f.read())
    st = os.stat(EMU)
    h.update(('%s %d %d' % (MODEL, st.st_size, int(st.st_mtime))).encode())
    d = os.path.join(os.path.dirname(os.path.abspath(EMU)), 'states')
    os.makedirs(d, exist_ok=True)
    name = '%s-%s.state' % (MODEL, h.hexdigest()[:16])
    for old in os.listdir(d):
        if old.startswith(MODEL + '-') and old not in (name, name[:-len('.state')] + '.img'):
            try:
                os.remove(os.path.join(d, old))
            except OSError:
                pass
    return os.path.join(d, name)


def emu_roms(emu):
    # Compass-Emu's roms folder (next to it): unpack MAME's sets there once
    import zipfile
    dst = os.path.join(os.path.dirname(os.path.abspath(emu)), 'roms')
    os.makedirs(dst, exist_ok=True)
    for z in ('grid1101.zip', 'grid1129.zip', 'grid1139.zip', 'grid_keyboard.zip'):
        src = os.path.join(ROMS, z)
        if os.path.exists(src):
            with zipfile.ZipFile(src) as f:
                for n in f.namelist():
                    if not os.path.exists(os.path.join(dst, n)):
                        f.extract(n, dst)


def gridfs(img, *args, text=False):
    cmd = [sys.executable, GRIDFS, img] + (['--text'] if text else []) + list(args)
    r = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if r.returncode:
        raise BuildError('gridfs %s failed:\n%s' % (args[0], r.stdout.decode('latin1').strip()))
    return r.stdout.decode('latin1')


def work_files(img):
    return subject_files(img, INSTALL_SUBJECT)


def subject_files(img, subj):
    names = []
    for line in gridfs(img, 'ls', '1:' + subj).splitlines():
        parts = line.split(None, 1)
        if len(parts) == 2:
            names.append(parts[1])
    return names


def has_work(img):
    return any(l.strip().lower() == INSTALL_SUBJECT.lower() + '~subject~' for l in gridfs(img, 'ls').splitlines())


def find_source(arg):
    arg = arg.rstrip('\\/')
    if os.path.isdir(arg):
        # a project folder: projects\hello -> projects\hello\hello.pas
        arg = os.path.join(arg, os.path.basename(os.path.abspath(arg)) + '.pas')
    stem, ext = os.path.splitext(arg)
    if ext.lower() not in LANGS and os.path.isfile(arg):
        # hello.run, hello.lst, screen.png ...: use the source next to it
        for e in LANGS:
            if os.path.isfile(stem + e):
                return os.path.abspath(stem + e)
        folder = os.path.dirname(os.path.abspath(arg))
        guess = os.path.join(folder, os.path.basename(folder) + '.pas')
        if os.path.isfile(guess):
            return guess
    if os.path.isfile(arg):
        return os.path.abspath(arg)
    guess = os.path.join(ROOT, 'projects', arg, arg + '.pas')
    if os.path.isfile(guess):
        return guess
    raise BuildError('Cannot find %s (or %s).' % (arg, guess))


def split_source(src):
    name, ext = os.path.splitext(os.path.basename(src))
    lang = LANGS.get(ext.lower())
    if not lang:
        raise BuildError('%s files are not supported yet (only %s).' % (ext, ', '.join(LANGS)))
    if not re.fullmatch(r'[A-Za-z][A-Za-z0-9_]{0,30}', name):
        raise BuildError('Name the file with letters, digits and _ only, e.g. hello.pas.')
    return name, lang


def debug_line(l, n):
    """A build script line for a debug build: DEBUG for the compiler and assembler, a link
    map from the linker (the code is the same as without)."""
    if re.search(r'Programs`(Pascal|Asm) ', l):
        return l + ' debug'
    if 'Programs`Link ' in l:
        return l.replace(' purge ', ' purge print(%sProjects`%s~MP1~) ' % (HD, n))
    return l


def build(src, use_state=True, debug=False):
    name, lang = split_source(src)
    proj = os.path.dirname(src)
    fmt = lambda s: s.format(n=name, hd=HD)
    mods = asm_modules(proj) if lang is LANGS['.pas'] else []
    pmods = pas_modules(proj, name) if lang is LANGS['.pas'] else []
    outputs = list(lang['outputs'])
    script_lines = [fmt(l) for l in lang['script']]
    if mods or pmods:
        outputs += [m + o for m in mods for o in ('.asm~Obj~', '.asm~Lst~')]
        outputs += [m + o for m in pmods for o in ('.pas~Obj~', '.pas~Lst~')]
        script_lines = [l.format(n=name, hd=HD,
                                 asm='\n'.join('%sPrograms`Asm %sProjects`%s.asm~Text~' % (HD, HD, m) for m in mods),
                                 pas='\n'.join('%sPrograms`Pascal %sProjects`%s.pas~Text~' % (HD, HD, m) for m in pmods),
                                 # Pascal modules by their short names (the prefix is Projects),
                                 # to keep the Link line short
                                 objs=''.join('%s.pas~obj~, ' % m for m in pmods) +
                                      ''.join('%sProjects`%s.asm~obj~, ' % (HD, m) for m in mods))
                        for l in ASM_SCRIPT]
        script_lines = [l for l in script_lines if l]
    if debug:
        script_lines = [x for l in script_lines for x in [debug_line(m, name) for m in l.split('\n')]]
    # A file named norun in the project folder: build only, don't start
    # the program (a game that never quits would keep MAME busy until
    # the time limit, and nothing comes back before that).
    norun = os.path.exists(os.path.join(proj, 'norun'))
    if norun:
        run_line = fmt('{hd}Projects`{n}')
        script_lines = [l for l in script_lines if l != run_line]
    tmp = tempfile.mkdtemp(prefix='gridbuild-')
    try:
        img = os.path.join(tmp, 'build.img')
        use_emu = os.path.exists(EMU)
        if not use_emu and not os.path.exists(MAME):
            raise BuildError('Cannot find %s or %s.' % (EMU, MAME))
        # Compass-Emu saved state: booted to the prompt once per disk, model and compass.exe,
        # kept with the prepared disk it booted from; each build starts from the pair (the
        # files below are added to the disk afterwards)
        state = emu_state() if use_emu and use_state else None
        base = state[:-len('.state')] + '.img' if state else None
        resumed = state is not None and os.path.exists(state) and os.path.exists(base)
        print('Copying the disk ...')
        if resumed:
            shutil.copyfile(base, img)
        else:
            shutil.copyfile(DISK, img)
            # the GRiD-OS build for the model that runs the build: 113x for the 1131/1137/1139
            # (and MAME's 1139), 11xx for the others
            wide = MODEL in ('1131', '1137', '1139') or not use_emu
            r = subprocess.run([sys.executable, os.path.join(HERE, 'gridmodel.py'), '1139' if wide else '1101', img, '--no-check'],
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            if r.returncode:
                raise BuildError(r.stdout.decode('latin1').strip())
            # boot straight to the command line instead of the Executive
            devexec = os.path.join(tmp, 'devexec.bin')
            gridfs(img, 'get', '1:Programs:DevelopmentExecutive~Run~', devexec)
            gridfs(img, 'put', devexec, '1:Programs:Executive~Run~')
            if state:
                if os.path.exists(state):
                    os.remove(state)
                shutil.copyfile(img, base)
        if not has_work(img):
            gridfs(img, 'mksubject', '1:Projects')
        before = {f.upper() for f in work_files(img)}
        for out in outputs:
            if fmt(out).upper() in before:
                gridfs(img, 'rm', '1:Projects:' + fmt(out))
        gridfs(img, 'put', src, '1:Projects:' + name + lang['kind'], text=True)
        for m in mods:
            gridfs(img, 'put', os.path.join(proj, m + '.asm'), '1:Projects:%s.asm~Text~' % m, text=True)
        for m in pmods:
            gridfs(img, 'put', os.path.join(proj, m + '.pas'), '1:Projects:%s.pas~Text~' % m, text=True)
        script = os.path.join(tmp, 'build.com')
        with open(script, 'w') as f:
            f.write('\n'.join(script_lines) + '\n')
        gridfs(img, 'put', script, '1:Programs:build~Com~', text=True)
        before = {f.upper() for f in work_files(img)}

        bubble = os.path.join(tmp, 'b.bubble')
        with open(bubble, 'wb') as f:
            f.write(bytes(393216))
        print('Compiling %s on the GRiD ...' % ', '.join([os.path.basename(src)] + [m + '.pas' for m in pmods] +
                                                     [m + '.asm' for m in mods]))
        if use_emu:
            emu_roms(EMU)
            emu_rom_dir = os.path.join(os.path.dirname(os.path.abspath(EMU)), 'roms')
            os.makedirs(os.path.join(tmp, 'snap'))
            r = subprocess.run([EMU, 'do', '--model', MODEL, '--hd', img, '--bubble', bubble, '--roms', emu_rom_dir,
                                '--shot', os.path.join(tmp, 'snap', 'screen.png'),
                                '--max-seconds', SECONDS] + (['--state', state] if state else []) +
                               (['--exact'] if os.environ.get('GRID_EXACT') else []) + ['Do build'],
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        else:
            r = subprocess.run([MAME, 'grid1139', '-rompath', ROMS,
                                '-ieee_grid2', 'grid2101_hdd', '-hard', img, '-mbm', bubble,
                                '-video', 'none', '-sound', 'none', '-nothrottle',
                                '-seconds_to_run', SECONDS, '-skip_gameinfo',
                                '-cfg_directory', os.path.join(tmp, 'cfg'),
                                '-nvram_directory', os.path.join(tmp, 'nv'),
                                '-snapshot_directory', os.path.join(tmp, 'snap'),
                                '-autoboot_script', LUA],
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        log = r.stdout.decode('latin1')
        if os.environ.get('GRID_SHOWLOG'):
            print(log.strip())
        finished = 'GRIDBUILD done' in log

        snaps = []
        for d, _, files in os.walk(os.path.join(tmp, 'snap')):
            snaps += [os.path.join(d, f) for f in files if f.endswith('.png')]
        if snaps:
            save_screen(sorted(snaps)[-1], proj)

        if resumed:
            # the GRiD resumed with files added under it: make sure it kept the disk sound
            chk = subprocess.run([sys.executable, GRIDFS, img, 'check'], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            if chk.returncode:
                print('The disk check failed after starting from the saved state; building again with a full boot.')
                os.remove(state)
                os.remove(base)
                shutil.rmtree(tmp, ignore_errors=True)
                return build(src, use_state=False)
        after = work_files(img)
        lst = os.path.join(proj, name + '.lst')
        errors, listing = [], False
        if fmt(lang['listing']).upper() in {f.upper() for f in after}:
            gridfs(img, 'get', '1:Projects:' + fmt(lang['listing']), lst, text=True)
            listing = True
            with open(lst, encoding='latin1') as f:
                lines = f.read().splitlines()
            errors = [l.rstrip() for l in lines if l.startswith('***')]
            summary = [l.strip() for l in lines if 'Errors Detected' in l]
        pas_failed = []
        for m in pmods:
            if (m + '.pas~Lst~').upper() not in {f.upper() for f in after}:
                pas_failed.append(m + '.pas')
                continue
            plst = os.path.join(proj, m + '.lst')
            gridfs(img, 'get', '1:Projects:%s.pas~Lst~' % m, plst, text=True)
            with open(plst, encoding='latin1') as f:
                plines = f.read().splitlines()
            perr = [l.rstrip() for l in plines if l.startswith('***')]
            psum = [l.strip() for l in plines if 'Errors Detected' in l]
            errors += ['%s.pas: %s' % (m, l) for l in perr]
            if psum:
                errors.append('%s.pas: %s' % (m, psum[-1]))
            if perr:
                pas_failed.append(m + '.pas')
        asm_errors, asm_failed = [], []
        for m in mods:
            if (m + '.asm~Lst~').upper() not in {f.upper() for f in after}:
                asm_failed.append(m + '.asm')
                continue
            alst = os.path.join(proj, m + '.asm.lst')
            gridfs(img, 'get', '1:Projects:%s.asm~Lst~' % m, alst, text=True)
            with open(alst, encoding='latin1') as f:
                alines = f.read().splitlines()
            asm_errors += ['%s.asm: %s' % (m, l.strip()) for l in alines if '***' in l]
            done = [l.strip() for l in alines if 'ASSEMBLY COMPLETE' in l.upper()]
            if done:
                asm_errors.append('%s.asm: %s' % (m, done[-1]))
            if not done or 'NO ERRORS' not in done[-1].upper():
                asm_failed.append(m + '.asm')
        run = os.path.join(proj, name + '.run')
        built = fmt('{n}~Run~').upper() in {f.upper() for f in after}
        if os.path.exists(run):
            os.remove(run)
        if built:
            gridfs(img, 'get', '1:Projects:' + fmt('{n}~Run~'), run)

        if debug:
            # symbols for Compass-Emu: the object files and the link map
            for f in os.listdir(proj):
                if f.lower().endswith(('.pas.obj', '.asm.obj')) or f.lower() == name.lower() + '.map':
                    os.remove(os.path.join(proj, f))
            for f in after:
                if f.lower().endswith('~obj~'):
                    gridfs(img, 'get', '1:Projects:' + f, os.path.join(proj, re.sub(r'~obj~$', '.obj', f, flags=re.I).lower()))
                elif f.upper() == fmt('{n}~MP1~').upper():
                    gridfs(img, 'get', '1:Projects:' + f, os.path.join(proj, name + '.map'), text=True)
        known = {fmt(o).upper() for o in outputs}
        new = [f for f in after if f.upper() not in before and f.upper() not in known]
        if new:
            outdir = os.path.join(proj, 'output')
            os.makedirs(outdir, exist_ok=True)
            for f in new:
                textual = '~text~' in f.lower()
                base = re.sub(r'~[^~]*~$', '', f) if textual else f
                host = re.sub(r'[\\/:*?"<>|~]+', '.', base).strip('.') + ('.txt' if textual else '')
                gridfs(img, 'get', '1:Projects:' + f, os.path.join(outdir, host), text=textual)

        print()
        for e in asm_errors:
            print(e)
        for m in asm_failed:
            if not os.path.exists(os.path.join(proj, m + '.lst')):
                print('The assembler did not finish %s. Look at screen.png.' % m)
        if asm_failed:
            if os.path.exists(run):
                os.remove(run)
            print('BUILD FAILED in %s. Errors are in the .asm.lst file next to it.' % ', '.join(asm_failed))
            return 1
        if pas_failed:
            for e in errors:
                print(e)
            if os.path.exists(run):
                os.remove(run)
            print('BUILD FAILED in %s. Errors are in the .lst file next to it '
                  '(no .lst: look at screen.png).' % ', '.join(pas_failed))
            return 1
        if not finished:
            print('%s stopped at its time limit (%s emulated seconds) before the build '
                  'was done. Set GRID_SECONDS higher.' % ('Compass-Emu' if use_emu else 'MAME', SECONDS))
            if use_emu:
                print(log.strip())
        if not listing:
            print('The compiler did not produce a listing. Look at screen.png.')
            if use_emu and MODEL in ('1101', '1121', '1131'):
                print('The %s has 256K of RAM, too little for the compiler: build on a 512K model '
                      '(1129, the default, 1109 or 1139).' % MODEL)
            return 1
        for e in errors:
            print(e)
        if summary:
            print(summary[-1])
        if not built:
            print('BUILD FAILED. Errors are in %s; linker messages are in screen.png.' % lst)
            return 1
        print('BUILD OK: %s' % run)
        if debug:
            print('Symbols saved (.obj files): in Compass-Emu use Debug > Load program symbols and '
                  'pick %s' % os.path.abspath(proj))
        if new:
            print('Your program wrote: ' + ', '.join(new) + '  (copied to output\\)')
        if norun:
            print('(norun: the program was not started)')
        elif not finished:
            print('Note: your program was still running when the time ran out '
                  '(waiting for a key?). screen.png shows where it was.')
        print('To try it on the GRiD, use run.bat.')
        return 0
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def mame_running():
    if os.name != 'nt':
        return False
    r = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq grid.exe'], stdout=subprocess.PIPE)
    return b'grid.exe' in r.stdout.lower()


def install(src):
    name, lang = split_source(src)
    run = os.path.join(os.path.dirname(src), name + '.run')
    if not os.path.exists(run):
        raise BuildError('There is no %s yet. Build it first with build.bat.' % run)
    if mame_running():
        raise BuildError('Close MAME first: changing the disk while MAME has it open can damage it.')
    subj = INSTALL_SUBJECT
    if not any(l.strip().lower() == subj.lower() + '~subject~' for l in gridfs(DISK, 'ls').splitlines()):
        gridfs(DISK, 'mksubject', '1:' + subj)
    gridfs(DISK, 'put', src, '1:%s:%s%s' % (subj, name, lang['kind']), text=True)
    gridfs(DISK, 'put', run, '1:%s:%s~Run~' % (subj, name))
    print('%s is now in the %s subject of your merged disk.' % (name, subj))
    return 0


def main(argv):
    if len(argv) != 2 or argv[0] not in ('build', 'debug', 'install'):
        print(__doc__)
        return 2
    try:
        src = find_source(argv[1])
        if argv[0] == 'install':
            return install(src)
        return build(src, debug=argv[0] == 'debug')
    except BuildError as e:
        print(e)
        return 1


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
