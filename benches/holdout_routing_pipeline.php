<?php

// PGO holdout: this workload is intentionally excluded from the bench_* and
// corpus_* training globs. It combines typed calls, object dispatch, dynamic
// string-key array access and branch-heavy aggregation in a previously unseen
// program shape.

class HoldoutRiskModel
{
    public function score(int $latency, int $bytes, string $route): int
    {
        $score = intdiv(($latency * 17) + $bytes, 13) + strlen($route);
        if ($route == 'write') {
            $score = $score + 37;
        } elseif ($route == 'delete') {
            $score = $score + 83;
        }
        if ($latency >= 300) {
            $score = $score + 101;
        }
        return $score;
    }
}

class HoldoutAdmissionGate
{
    public function accepted(int $score, int $sequence): int
    {
        if (($score % 11) == 0 || ($sequence % 17) == 0) {
            return 0;
        }
        return 1;
    }
}

function runHoldoutRoutingPipeline(int $iterations): array
{
    $model = new HoldoutRiskModel();
    $gate = new HoldoutAdmissionGate();
    $totals = ['read' => 0, 'write' => 0, 'delete' => 0];
    $accepted = ['read' => 0, 'write' => 0, 'delete' => 0];
    $rejected = 0;

    for ($i = 0; $i < $iterations; $i++) {
        $selector = $i % 10;
        if ($selector < 6) {
            $route = 'read';
        } elseif ($selector < 9) {
            $route = 'write';
        } else {
            $route = 'delete';
        }

        $latency = 20 + (($i * 17) % 400);
        $bytes = 128 + (($i * 73) % 8192);
        $score = $model->score($latency, $bytes, $route);
        $totals[$route] = $totals[$route] + $score;

        $isAccepted = $gate->accepted($score, $i);
        if ($isAccepted == 1) {
            $accepted[$route] = $accepted[$route] + 1;
        } else {
            $rejected = $rejected + 1;
        }
    }

    return [
        'readTotal' => $totals['read'],
        'writeTotal' => $totals['write'],
        'deleteTotal' => $totals['delete'],
        'readAccepted' => $accepted['read'],
        'writeAccepted' => $accepted['write'],
        'deleteAccepted' => $accepted['delete'],
        'rejected' => $rejected,
    ];
}

$start = microtime(true);
$result = runHoldoutRoutingPipeline(750000);
$elapsed = microtime(true) - $start;
echo $result['readTotal'] . ','
    . $result['writeTotal'] . ','
    . $result['deleteTotal'] . ','
    . $result['readAccepted'] . ','
    . $result['writeAccepted'] . ','
    . $result['deleteAccepted'] . ','
    . $result['rejected'] . '|'
    . $elapsed;
