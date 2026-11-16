import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('backfill', Path(__file__).with_name('backfill-provenance.py'))
backfill = importlib.util.module_from_spec(spec)
spec.loader.exec_module(backfill)


class MigrationTests(unittest.TestCase):
    def test_local_evidence_preserves_user_values_and_is_idempotent(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            folder = root / 'library/pack/materials/Stone'
            folder.mkdir(parents=True)
            (folder.parent.parent / 'pack.json').write_text(json.dumps({'name': 'Surfaces'}))
            (folder / 'provenance.json').write_text(json.dumps({'provider': 'Studio', 'source': 'https://example.test/stone', 'license': 'CC0-1.0'}))
            manifest = {'assets': [{'id': 'stone', 'kind': 'material', 'pack': 'pack', 'name': 'Stone', 'category': 'c', 'maps': []}]}
            user = {'favorite': True, 'tags': ['custom'], 'collections': ['Game'], 'provenance': {'publisher': 'User publisher'}}
            cache = {'libraryRoot': str(root), 'rigs': [{'id': 'rig'}], 'assets': [{**manifest['assets'][0], 'user': user, 'thumb': {'state': 'ready'}}]}
            migrated, indexed, report = backfill.prepare(root, manifest, cache, {})
            self.assertEqual(indexed['assets'][0]['user']['provenance']['publisher'], 'User publisher')
            self.assertEqual(indexed['assets'][0]['user']['provenance']['url'], 'https://example.test/stone')
            self.assertNotIn('filesystemPath', indexed['assets'][0]['user']['provenance'])
            self.assertEqual(indexed['assets'][0]['user']['tags'], ['custom', 'source: Surfaces', 'publisher: User publisher', 'license: CC0 1.0'])
            self.assertEqual(indexed['assets'][0]['thumb'], cache['assets'][0]['thumb'])
            self.assertEqual(indexed['rigs'], cache['rigs'])
            self.assertTrue(indexed['assets'][0]['user']['favorite'])
            self.assertEqual(indexed['assets'][0]['user']['collections'], ['Game'])
            again, again_cache, report = backfill.prepare(root, migrated, indexed, {})
            self.assertEqual((again, again_cache), (migrated, indexed))
            self.assertEqual(report['updatedRecordsIncludingVariants'], 0)

    def test_backups_and_concurrent_change_leave_original_files_intact(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = [Path(directory) / name / 'catalog.json' for name in ['manifest', 'cache']]
            for path in paths:
                path.parent.mkdir()
                path.write_bytes(b'{"old":true}')
            original = [path.read_bytes() for path in paths]
            backups = backfill.commit_pair(paths, original, [{'new': True}, {'new': True}], 'test')
            self.assertTrue(all(Path(path).read_bytes() == original[0] for path in backups))
            current = [path.read_bytes() for path in paths]
            paths[1].write_bytes(b'{"concurrent":true}')
            with self.assertRaises(RuntimeError):
                backfill.commit_pair(paths, current, [{}, {}], 'conflict')
            self.assertEqual(paths[0].read_bytes(), current[0])
            self.assertEqual(paths[1].read_bytes(), b'{"concurrent":true}')


if __name__ == '__main__':
    unittest.main()
