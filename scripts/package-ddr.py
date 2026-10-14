"""Package the built DDR DLL and independent Street frame with repository defaults."""
import pathlib
import zipfile

root = pathlib.Path(__file__).resolve().parent.parent
output = root / 'dist' / 'bilimani-ddr.zip'
files = [(root / 'target/release/bilimani_ddr.dll', 'bilimani_ddr.dll')]
for name in ['bilimani_ddr.json', 'bilimani_ddr_aliases.csv']:
    files.append((root / 'config' / name, name))
page = root / 'dist/web/ddr/street'
files.append((page / 'index.html', 'bilimani_web/ddr/street/index.html'))
files.append((root / 'web/apps/ddr/config.json', 'bilimani_web/ddr/street/config.json'))
for source in sorted((page / 'assets').rglob('*')):
    if source.is_file():
        files.append((source, 'bilimani_web/ddr/street/' + source.relative_to(page).as_posix()))
for source, _ in files:
    if not source.is_file():
        raise SystemExit(f'Missing {source}; build Rust release and frontend first')
output.parent.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
    for source, destination in files:
        archive.write(source, destination)
print(f'Packaged: {output}')
