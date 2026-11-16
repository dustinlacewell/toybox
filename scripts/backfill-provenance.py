"""Backfill legacy provenance from local evidence, preserving media and user metadata.

Dry run by default. --evidence accepts a JSON mapping of pack slugs to explicitly
known provenance (for example, original archive paths recorded during import).
"""
import argparse
import collections
import copy
import datetime
import json
import os
from pathlib import Path
import re

FIELDS = ('filesystemPath', 'source', 'publisher', 'url', 'license')


def read_json(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def nonempty(value):
    return {key: value[key].strip() for key in FIELDS
            if isinstance(value.get(key), str) and value[key].strip()}


def local_evidence(root, entry, explicit):
    folder = root / 'library' / entry['pack']
    if not folder.resolve().is_relative_to(root.resolve()):
        raise ValueError('Pack path escapes library')
    pack = read_json(folder / 'pack.json') if (folder / 'pack.json').is_file() else {}
    found = {}
    if pack.get('name'):
        found['source'] = pack['name']
    for key in ('publisher', 'url', 'license'):
        if pack.get(key):
            found[key] = pack[key]
    if str(pack.get('source', '')).startswith(('https://', 'http://')):
        found.setdefault('url', pack['source'])
    source = folder / 'SOURCE.txt'
    if source.is_file():
        labels = dict(re.findall(r'^(Source|Creator|Publisher|License):\s*(.+)$', source.read_text(encoding='utf-8-sig'), re.M))
        if labels.get('Source', '').startswith(('https://', 'http://')):
            found.setdefault('url', labels['Source'])
        if labels.get('Publisher') or labels.get('Creator'):
            found.setdefault('publisher', labels.get('Publisher') or labels['Creator'])
        if labels.get('License'):
            found.setdefault('license', labels['License'].split(', see ')[0])
    if entry.get('kind') == 'material':
        path = folder / 'materials' / entry['name'] / 'provenance.json'
        if not path.resolve().is_relative_to(folder.resolve()):
            raise ValueError('Material path escapes pack')
        if path.is_file():
            metadata = read_json(path)
            for key, source_key in [('publisher', 'provider'), ('url', 'source'), ('license', 'license')]:
                if metadata.get(source_key):
                    found[key] = metadata[source_key]
    found.update(nonempty(explicit.get(entry['pack'], {})))
    if found.get('license') in ('CC0-1.0', 'CC0 1.0'):
        found['license'] = 'CC0 1.0'
    return nonempty(found)


def walk(entries):
    for entry in entries:
        yield entry
        yield from walk(entry.get('variants', []))


def prepare(root, manifest, cache, explicit):
    manifest, cache = copy.deepcopy(manifest), copy.deepcopy(cache)
    if Path(cache['libraryRoot']).resolve() != root.resolve():
        raise ValueError('Cache belongs to another library')
    cached = {entry['id']: entry for entry in walk(cache['assets'])}
    provenance_by_id = {}
    for entry in walk(manifest['assets']):
        previous = cached.get(entry['id'], {}).get('user', {}).get('provenance') or {}
        provenance = {**local_evidence(root, entry, explicit), **nonempty(previous), **nonempty(entry.get('provenance') or {})}
        if provenance:
            entry['provenance'] = provenance
            provenance_by_id[entry['id']] = provenance
    changed = 0
    coverage = collections.Counter()
    missing = {}
    for entry in walk(cache['assets']):
        provenance = provenance_by_id.get(entry['id'])
        before = copy.deepcopy(entry['user'])
        if provenance:
            entry['user']['provenance'] = provenance
            tags = entry['user']['tags']
            for field in ('source', 'publisher', 'license'):
                if field in provenance:
                    tag = f'{field}: {provenance[field]}'
                    if tag.casefold() not in {tag.casefold() for tag in tags}:
                        tags.append(tag)
        changed += before != entry['user']
    for entry in cache['assets']:
        provenance = entry['user'].get('provenance') or {}
        for field in FIELDS:
            if provenance.get(field):
                coverage[field] += 1
            else:
                missing.setdefault(entry['pack'], collections.Counter())[field] += 1
    return manifest, cache, {'totalItems': len(cache['assets']), 'updatedRecordsIncludingVariants': changed,
                             'fieldCoverage': dict(coverage), 'missingByPack': {key: dict(value) for key, value in missing.items()}}


def commit_pair(paths, originals, updated, stamp):
    """Back up exact bytes and refuse concurrent changes before either replace."""
    for path, original in zip(paths, originals):
        if path.read_bytes() != original:
            raise RuntimeError(f'{path} changed; retry when import is idle')
    backups = [path.with_name(f'catalog-before-provenance-{stamp}.json') for path in paths]
    for backup, original in zip(backups, originals):
        with backup.open('xb') as out:
            out.write(original)
    prepared = [json.dumps(doc, ensure_ascii=False, indent=2).encode('utf-8') + b'\n' for doc in updated]
    written = []
    try:
        for path, original, data in zip(paths, originals, prepared):
            temporary = path.with_name(f'.provenance-{stamp}.tmp')
            with temporary.open('xb') as out:
                out.write(data)
            if path.read_bytes() != original:
                temporary.unlink()
                raise RuntimeError(f'{path} changed; backed up but not overwritten')
            os.replace(temporary, path)
            written.append((path, original, data))
    except Exception:
        for path, original, data in reversed(written):
            if path.read_bytes() == data:
                temporary = path.with_name(f'.provenance-rollback-{stamp}.tmp')
                temporary.write_bytes(original)
                os.replace(temporary, path)
        raise
    return [str(path) for path in backups]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('library', type=Path)
    parser.add_argument('cache', type=Path)
    parser.add_argument('--evidence', type=Path)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    paths = [args.library / '_library_config/catalog.json', args.cache]
    originals = [path.read_bytes() for path in paths]
    documents = [json.loads(data.decode('utf-8-sig')) for data in originals]
    evidence = read_json(args.evidence) if args.evidence else {}
    manifest, cache, report = prepare(args.library, *documents, evidence)
    changed = [manifest, cache] != documents
    report.update({'applied': args.apply and changed, 'changed': changed})
    if args.apply and changed:
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
        report['backups'] = commit_pair(paths, originals, [manifest, cache], stamp)
        if args.evidence:
            report['explicitEvidence'] = evidence
        report_path = args.library / '_library_config' / f'provenance-backfill-{stamp}.json'
        report_path.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        report['reportPath'] = str(report_path)
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
