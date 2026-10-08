"""Build an additive mecha style ZIP without replacing the existing card style."""
import argparse
import pathlib
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=pathlib.Path, default=ROOT / 'dist/bilimani-green-room.zip')
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.output, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        # Every style owns its assets; no root files or sibling styles are replaced.
        style = ROOT / 'web/mecha'
        for source in sorted(style.rglob('*')):
            if source.is_file():
                archive.write(source, pathlib.PurePosixPath('bilimani_web/mecha') / source.relative_to(style).as_posix())
        archive.write(ROOT / 'docs/stream-frame.md', 'README.md')
        archive.write(ROOT / 'LICENSE', 'LICENSE')
    print(f'Packaged: {args.output}')


if __name__ == '__main__':
    main()
