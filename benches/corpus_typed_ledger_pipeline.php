<?php

// Contract-typed counterpart of corpus_ledger_pipeline.php. Business work and
// object lifetime are identical; only stable method/function boundaries carry
// declarations.

class TypedLedgerFeePolicy
{
    public function fee(int $amount, int $priority): int
    {
        $fee = intdiv($amount * 35, 1000);
        if ($priority == 3) {
            $fee = $fee + 11;
        }
        return $fee;
    }
}

class TypedLedgerAggregate
{
    public $count = 0;
    public $amountTotal = 0;
    public $feeTotal = 0;
    public $largeCount = 0;

    public function record(int $amount, int $fee, int $large)
    {
        $this->count = $this->count + 1;
        $this->amountTotal = $this->amountTotal + $amount;
        $this->feeTotal = $this->feeTotal + $fee;
        $this->largeCount = $this->largeCount + $large;
    }

    public function summary(): array
    {
        return [
            'count' => $this->count,
            'amount' => $this->amountTotal,
            'fees' => $this->feeTotal,
            'large' => $this->largeCount,
        ];
    }
}

function runTypedLedgerPipeline(int $iterations): array
{
    $policy = new TypedLedgerFeePolicy();
    $ledger = new TypedLedgerAggregate();

    for ($i = 0; $i < $iterations; $i++) {
        $amount = 1000 + (($i % 400) * 75);
        $priority = $i % 4;
        $large = 0;
        if ($amount >= 20000) {
            $large = 1;
        }
        $fee = $policy->fee($amount, $priority);
        $ledger->record($amount, $fee, $large);
    }

    return $ledger->summary();
}

$start = microtime(true);
$result = runTypedLedgerPipeline(500000);
$elapsed = microtime(true) - $start;
echo $result['count'] . ',' . $result['amount'] . ',' . $result['fees'] . ',' . $result['large'] . '|' . $elapsed;
