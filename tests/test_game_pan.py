# SPDX-License-Identifier: GPL-3.0-only
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
spec=importlib.util.spec_from_file_location("pan",Path(__file__).parents[1]/"tools/pan-game-camera.py")
pan=importlib.util.module_from_spec(spec);spec.loader.exec_module(pan)
class PanTests(unittest.TestCase):
    def test_samples_and_bounded_packets(self):
        sent=[];rows=[]
        with patch.object(pan.time,"sleep"):
            pan.run(lambda x,y:sent.append((x,y)),lambda:{"fg_active":1},9,2,0,1/60,rows)
        self.assertEqual(sent,[(2,0)]*9)
        self.assertEqual([r["phase"] for r in rows],["before","pan","pan","pan","after"])
    def test_focus_loss_stops_without_more_packets(self):
        sent=[];rows=[]
        def send(x,y):
            if len(sent)==3:raise RuntimeError("foreground lost")
            sent.append((x,y))
        with patch.object(pan.time,"sleep"),self.assertRaisesRegex(RuntimeError,"foreground lost"):
            pan.run(send,lambda:{},120,2,0,1/60,rows)
        self.assertEqual(len(sent),3)
        self.assertNotIn("after",[r["phase"] for r in rows])
if __name__=="__main__":unittest.main()
