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
        style = ROOT / 'dist/web/iidx/mecha'
        if not (style / 'index.html').is_file():
            raise SystemExit('Run npm run build before packaging')
        sources = [style / 'index.html', *sorted((style / 'assets').rglob('*'))]
        for source in sources:
            if source.is_file():
                archive.write(source, pathlib.PurePosixPath('bilimani_web/iidx/mecha') / source.relative_to(style).as_posix())
        archive.write(ROOT / 'web/apps/iidx/config.json', 'bilimani_web/iidx/mecha/config.json')
    print(f'Packaged: {args.output}')


if __name__ == '__main__':
    main()
