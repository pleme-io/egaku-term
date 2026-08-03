// Proves the bumped pin actually exposes the four new modules to a consumer.
#[test]
fn new_egaku_widgets_are_reachable_from_egaku_term() {
    let d = egaku::DiffView::parse("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n");
    assert_eq!(d.files().len(), 1);
    let mut t = egaku::TextArea::with_text("a\nb");
    t.move_up();
    assert_eq!(t.row_count(), 2);
    let v = egaku::TextView::from_text("日本語", 4, 10);
    assert_eq!(v.total_rows(), 2, "display-width wrap reached the consumer");
    let (diff, _, _) = egaku::chigai::diff_lines("a\nb", "a\nc");
    assert_eq!(diff.stats(), (1, 1));
}
