mod common;

use common::run_php;

#[test]
fn inner_unset_does_not_rewind_an_unrelated_detached_reference_loop() {
    // Exercise small, linear and indexed child arrays in ordinary and
    // generator frames. The visit bound makes a repeated outer item a finite
    // assertion failure instead of leaving the test runner in an endless loop.
    let source = r#"<?php
class NestedRewrite {
    public array $states;
    public function rewrite(array $states, int $width) {
        $this->states = $states;
        $visited = 0;
        foreach ($this->states as &$row) {
            $replacement = [];
            foreach ($row as $key => $value) {
                if (++$visited > 2 * $width) { echo 'repeated|'; return; }
                unset($row[$key]);
                $replacement[$key] = $value + 10;
            }
            $row = $replacement;
        }
        echo $width, ':', $visited, ':', array_sum($this->states['left']),
            ':', array_sum($this->states['right']), '|';
        __FINISH__
    }
}
foreach ([3, 6, 12] as $width) {
    $row = [];
    for ($i = 0; $i < $width; $i++) { $row['k' . $i] = $i; }
    $rewriter = new NestedRewrite;
    __INVOKE__
}
"#;
    for generator in [false, true] {
        let (finish, invoke) = if generator {
            (
                "yield 'done';",
                "$g = $rewriter->rewrite(['left' => $row, 'right' => $row], $width); echo $g->current(), '|'; $g->next();",
            )
        } else {
            (
                "return 'done';",
                "echo $rewriter->rewrite(['left' => $row, 'right' => $row], $width), '|';",
            )
        };
        assert_eq!(
            run_php(
                &source
                    .replace("__FINISH__", finish)
                    .replace("__INVOKE__", invoke)
            ),
            "3:6:33:33|done|6:12:75:75|done|12:24:186:186|done|",
            "generator={generator}"
        );
    }
}
