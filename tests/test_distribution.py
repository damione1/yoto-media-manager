"""Offline inventory/DMG regressions; no signing or release action."""
import importlib.util,pathlib,tempfile,unittest
spec=importlib.util.spec_from_file_location('distribution_utils',pathlib.Path(__file__).resolve().parents[1]/'scripts/distribution_utils.py')
distribution=importlib.util.module_from_spec(spec);spec.loader.exec_module(distribution)
class DistributionTests(unittest.TestCase):
    def test_inventory_twice_excludes_itself_and_tracks_symlink_closure(self):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);(root/'ffmpeg').write_bytes(b'fixture');(root/'lib').mkdir();(root/'lib/a.dylib').write_bytes(b'library');(root/'lib/current.dylib').symlink_to('a.dylib')
            distribution.write_manifest(root,'aarch64-apple-darwin');first=(root/'manifest.json').read_bytes();distribution.write_manifest(root,'aarch64-apple-darwin');self.assertEqual(first,(root/'manifest.json').read_bytes())
            manifest=distribution.manifest(root,'aarch64-apple-darwin');self.assertNotIn('manifest.json',manifest['files']);self.assertEqual(manifest['links'],{'lib/current.dylib':'a.dylib'})
            (root/'bad').symlink_to('/usr/bin/true')
            with self.assertRaises(ValueError):distribution.manifest(root,'aarch64-apple-darwin')
    def test_dmg_staging_twice_is_fresh_after_interrupted_run(self):
        with tempfile.TemporaryDirectory() as directory:
            root=pathlib.Path(directory);app=root/'Yoto.app';app.mkdir();(app/'current').write_text('current')
            first=distribution.stage_dmg(app,root);old=pathlib.Path(first.name);(old/app.name/'obsolete').write_text('old')
            second=distribution.stage_dmg(app,root);new=pathlib.Path(second.name);self.assertNotEqual(old,new);self.assertFalse((new/app.name/'obsolete').exists());self.assertTrue((new/'Applications').is_symlink());self.assertEqual((new/'Applications').readlink(),pathlib.Path('/Applications'))
            first.cleanup();second.cleanup()
if __name__=='__main__':unittest.main()
