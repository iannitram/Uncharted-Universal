from paths import ROOT
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import content_profile

class ProfileTests(unittest.TestCase):
    def test_sfo_parameters_and_explicit_trial_profile(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/'sce_sys').mkdir();(p/'sce_sys/param.sfo').write_bytes(b'fixture')
            with patch.object(content_profile,'sfo',return_value={'USER_DEFINED_PARAM_1':13,'USER_DEFINED_PARAM_4':0xffffffff}):
                content_profile.prepare(p,p,'trial')
            self.assertEqual(struct.unpack('<8s5I',(p/'content.bin').read_bytes()),(b'BBCONT01',1,13,0,0,0xffffffff))

    def test_missing_param_1_is_the_title_number(self):
        for sfo_values, expected in (({'TITLE_ID': 'CUSA02320'}, 2320), ({'TITLE_ID': 'CUSA02343'}, 2343),
                                     ({'TITLE_ID': 'CUSA02320', 'USER_DEFINED_PARAM_1': 7}, 7),
                                     ({}, 0)):
            with tempfile.TemporaryDirectory() as tmp:
                p=Path(tmp);(p/'sce_sys').mkdir();(p/'sce_sys/param.sfo').write_bytes(b'fixture')
                with patch.object(content_profile,'sfo',return_value=sfo_values):
                    content_profile.prepare(p,p)
                self.assertEqual(struct.unpack('<8s5I',(p/'content.bin').read_bytes())[2],expected,sfo_values)

    def test_invalid_sfo_parameter_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp);(p/'sce_sys').mkdir();(p/'sce_sys/param.sfo').write_bytes(b'fixture')
            with patch.object(content_profile,'sfo',return_value={'USER_DEFINED_PARAM_1':'13'}):
                with self.assertRaisesRegex(ValueError,'invalid user-defined'):
                    content_profile.prepare(p,p)
