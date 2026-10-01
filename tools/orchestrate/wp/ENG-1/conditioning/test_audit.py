import unittest
from audit import compare


class Attribution(unittest.TestCase):
    def test_outside_support_is_blocked_even_for_one_ulp(self):
        before = [(0., 0., 0., 1.)] * 40
        after = before.copy()
        after[22] = (2**-149, 0., 0., 1.)
        self.assertEqual(compare(before, after, [10], 40)['outside_predicate'], [22])

    def test_radius_is_chebyshev_and_boundary_is_inclusive(self):
        before = [(0., 0., 0., 1.)] * 1600
        after = before.copy()
        after[21 * 40 + 21] = (0.2, 0., 0., 1.)
        report = compare(before, after, [10 * 40 + 10], 40)
        self.assertEqual(report['outside_predicate'], [])
        self.assertEqual(report['changed_pixels'], 1)
        self.assertEqual(report['max_encoded_delta'], 0.2)

    def test_neutral_path_cannot_allow_any_change(self):
        self.assertEqual(compare([(0., 0., 0., 1.)], [(1.,)*4], [], 1)['outside_predicate'], [0])


if __name__ == '__main__':
    unittest.main()
