mod common;

use common::run_php;

#[test]
fn inherited_methods_address_their_own_private_same_name_property() {
    assert_eq!(
        run_php(
            r#"<?php
class RootOutput {
    private string $output;

    public function __construct(string $output) {
        $this->output = $output;
    }

    public function root(): string {
        return $this->output;
    }
}

class StyledOutput extends RootOutput {
    private string $output;

    public function __construct(string $output) {
        parent::__construct($this->output = strtoupper($output));
    }

    public function styled(): string {
        return $this->output;
    }
}

class ToolOutput extends StyledOutput {
    public function __construct(string $output) {
        parent::__construct($output);
    }
}

$output = new ToolOutput('ok');
echo $output->root(), ':', $output->styled(), ':';
var_export(isset($output->missing));
unset($output->missing);
"#,
        ),
        "OK:OK:false",
    );
}
