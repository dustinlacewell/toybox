import importlib.util
import math
import pathlib
import struct
import unittest

spec = importlib.util.spec_from_file_location("synty_import", pathlib.Path(__file__).with_name("import-synty-animations.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ClipCuts(unittest.TestCase):
    def fixture(self, values, kind):
        binary = struct.pack("<3f", 0, 1, 2) + struct.pack("<" + "f" * len(values), *values)
        doc = {"buffers": [{"uri": "take.bin", "byteLength": len(binary)}],
               "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 12}, {"buffer": 0, "byteOffset": 12, "byteLength": len(binary) - 12}],
               "accessors": [{"bufferView": 0, "componentType": 5126, "type": "SCALAR", "count": 3, "min": [0], "max": [2]},
                             {"bufferView": 1, "componentType": 5126, "type": kind, "count": 3}],
               "animations": [{"samplers": [{"input": 0, "output": 1}], "channels": [{"sampler": 0, "target": {"node": 0, "path": "rotation" if kind == "VEC4" else "translation"}}]}]}
        return doc, binary

    def values(self, doc, binary, index, width):
        accessor = doc["accessors"][index]
        offset = doc["bufferViews"][accessor["bufferView"]]["byteOffset"]
        return [struct.unpack_from("<" + "f" * width, binary, offset + i * width * 4) for i in range(accessor["count"])]

    def test_cut_keeps_interior_keys_and_interpolates_exact_boundaries(self):
        doc, binary = self.fixture([0, 0, 0, 2, 4, 0, 4, 8, 0], "VEC3")
        result, binary = module.split_clips(doc, binary, [{"name": "Hit", "firstFrame": 15, "lastFrame": 45}], [(0, 60)], "Take")
        sampler = result["animations"][0]["samplers"][0]
        self.assertEqual(self.values(result, binary, sampler["input"], 1), [(0,), (.5,), (1,)])
        self.assertEqual(self.values(result, binary, sampler["output"], 3), [(1, 2, 0), (2, 4, 0), (3, 6, 0)])

    def test_rotation_boundary_uses_shortest_arc_and_unit_quaternion(self):
        doc, binary = self.fixture([0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, -1], "VEC4")
        result, binary = module.split_clips(doc, binary, [{"name": "Turn", "firstFrame": 15, "lastFrame": 45}], [(0, 60)], "Take")
        sample = self.values(result, binary, result["animations"][0]["samplers"][0]["output"], 4)[0]
        self.assertAlmostEqual(sample[1], math.sqrt(.5), places=6)
        self.assertAlmostEqual(sample[3], math.sqrt(.5), places=6)

    def test_constant_pose_retains_authored_joint_transforms(self):
        doc = {"nodes": [{"name": "Hip", "translation": [1, 2, 3]}], "skins": [{"joints": [0]}], "buffers": [{"uri": "pose.bin", "byteLength": 0}]}
        result, binary = module.pose_clip(doc, b"", {"name": "Pose", "firstFrame": 10, "lastFrame": 11})
        self.assertEqual(len(result["animations"][0]["channels"]), 3)
        self.assertEqual(self.values(result, binary, result["animations"][0]["samplers"][0]["output"], 3), [(1, 2, 3), (1, 2, 3)])


if __name__ == "__main__":
    unittest.main()
