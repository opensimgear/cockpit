import importlib.util,json,unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('unified_native_acceptance',Path(__file__).with_name('mapping-acceptance.py'))
acceptance=importlib.util.module_from_spec(spec);spec.loader.exec_module(acceptance)
class ProfileAcceptance(unittest.TestCase):
    def test_constant_graph_is_portable_schema2_without_provider_or_machine_assignments(self):
        document=acceptance.constant_profile()
        self.assertEqual(document['schemaVersion'],2)
        self.assertEqual(document['resources'],[])
        self.assertEqual(document['virtualOutputs'],[])
        self.assertEqual(len(document['bindings']),1)
        self.assertEqual(document['processingNodes'][0]['outputs'][0]['contract']['schema'],{'type':'boolean'})
        self.assertEqual(document['processingNodes'][0]['operation'],{'kind':'constant','value':True})
        self.assertNotIn('sourceId',json.dumps(document))
    def test_retired_negative_boundaries_fail_on_any_accidental_positive_reply(self):
        class Rejected:
            def request(self,*a,**k):return {'ok':False}
        acceptance.retired_boundaries(Rejected())
        class Accepted:
            def request(self,*a,**k):return {'ok':True}
        with self.assertRaises(AssertionError):acceptance.retired_boundaries(Accepted())
if __name__=='__main__':unittest.main()
