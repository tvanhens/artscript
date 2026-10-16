import unittest

from migrate_statement_heads import migrate


class MigrateStatementHeads(unittest.TestCase):
    def test_palette_rows_lead_with_ramp_or_color(self):
        src = (
            "palette garden\n"
            "  peach  ramp #f3a2a6 5        # 0 deep\n"
            "  leaf   ramp #7db956 5 hue:12\n"
            "  sky    ramp #1b1f3a #f2a65a 7\n"
            "  # a note\n"
            "  ink    #3d2632\n"
            "circle 4 fill:garden.ink\n"
        )
        out, notes = migrate(src)
        self.assertEqual(
            out,
            "palette garden\n"
            "  ramp peach #f3a2a6 5        # 0 deep\n"
            "  ramp leaf #7db956 5 hue:12\n"
            "  ramp sky #1b1f3a #f2a65a 7\n"
            "  # a note\n"
            "  color ink #3d2632\n"
            "circle 4 fill:garden.ink\n",
        )
        self.assertEqual(notes, [])
        self.assertEqual(migrate(out)[0], out, "a rerun changes nothing")

    def test_mask_colon_block_becomes_a_mask_block(self):
        src = "group feather:6\n  mask:\n    ellipse 150,140 at:330,440\n  circle 3\n"
        out, _ = migrate(src)
        self.assertEqual(out, "group feather:6\n  mask\n    ellipse 150,140 at:330,440\n  circle 3\n")

    def test_mask_colon_value_gets_a_space(self):
        out, _ = migrate("rect 40x40\n  mask:simplex(30) > .5\n")
        self.assertEqual(out, "rect 40x40\n  mask simplex(30) > .5\n")

    def test_mask_named_arg_moves_to_a_child_line(self):
        out, _ = migrate("group mask:@body a:.5\n  circle 4\nrect 2x2 mask:simplex(30) > .5 # soft\n")
        self.assertEqual(
            out,
            "group a:.5\n  mask @body\n  circle 4\nrect 2x2 # soft\n  mask simplex(30) > .5\n",
        )

    def test_let_fx_pair_is_renamed_and_the_fx_keyword_is_kept(self):
        src = (
            "each k in 0..3\n"
            "  let fx = x+rand(-12 4)\n"
            "  let fy = yc+1\n"
            "  curve fx-4,fy-3 fx+4,fy+3 color:fx\n"
            "circle 4\n"
            "  fx rim 4 color:p.rim\n"
        )
        out, _ = migrate(src)
        self.assertEqual(
            out,
            "each k in 0..3\n"
            "  let fiber_x = x+rand(-12 4)\n"
            "  let fiber_y = yc+1\n"
            "  curve fiber_x-4,fiber_y-3 fiber_x+4,fiber_y+3 color:fiber_x\n"
            "circle 4\n"
            "  fx rim 4 color:p.rim\n",
        )

    def test_top_level_fx_is_reported(self):
        out, notes = migrate("layer foil\nfx grade warm:.2\n")
        self.assertEqual(out, "layer foil\nfx grade warm:.2\n")
        self.assertEqual(notes, ["line 2: top-level `fx` has no draw to attach to"])


if __name__ == "__main__":
    unittest.main()
