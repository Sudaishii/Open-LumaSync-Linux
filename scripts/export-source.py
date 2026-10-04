#!/usr/bin/env python3
"""Export only public project sources, without checkout/history or local files."""
from pathlib import Path
import tarfile
import zipfile

PROJECT = Path(__file__).resolve().parent.parent
DESTINATION = PROJECT / 'dist'
TOP = 'snzhy-OpenSycnlights'
ROOT_FILES = ('README.md', 'INSTALL.md', 'USAGE.md', 'TROUBLESHOOTING.md', 'BUILD.md',
              'CONTRIBUTING.md', 'PUBLISHING.md', 'CHANGELOG.md', 'LICENSE', 'ATTRIBUTION.md',
              'STOCK-PROTOCOL.md', '.gitignore', 'install.sh', 'run.sh')
DIRECTORIES = ('.github', 'docs', 'ui', 'src-tauri', 'packaging', 'tests', 'scripts', 'gnome-extension')
SKIP_PARTS = {'target', 'gen', '__pycache__', 'node_modules', '.git'}


def public_sources():
    files = [PROJECT / name for name in ROOT_FILES]
    for name in DIRECTORIES:
        for path in (PROJECT / name).rglob('*'):
            relative = path.relative_to(PROJECT)
            if any(part in SKIP_PARTS for part in relative.parts):
                continue
            if path.is_file() and not path.is_symlink() and path.suffix not in ('.pyc', '.log'):
                files.append(path)
    return sorted(set(files))


def main():
    files = public_sources()
    for path in files:
        if not path.is_file():
            raise SystemExit(f'Missing required publication file: {path.relative_to(PROJECT)}')
    DESTINATION.mkdir(exist_ok=True)
    zip_path = DESTINATION / f'{TOP}-source.zip'
    tar_path = DESTINATION / f'{TOP}-source.tar.gz'
    with zipfile.ZipFile(zip_path, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for path in files:
            archive.write(path, f'{TOP}/{path.relative_to(PROJECT)}')
    with tarfile.open(tar_path, 'w:gz') as archive:
        for path in files:
            archive.add(path, arcname=f'{TOP}/{path.relative_to(PROJECT)}', recursive=False)
    print(f'Exported {len(files)} public source files:\n{zip_path}\n{tar_path}')


if __name__ == '__main__':
    main()
