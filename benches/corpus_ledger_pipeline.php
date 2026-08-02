<?php

// Independent stateful application corpus. Unlike the order pipeline this
// keeps one aggregate object alive, feeds a scalar method result into a
// property-mutating method, and carries no transient DTO/result array through
// the hot loop.

class LedgerFeePolicy
{
    public function fee($amount, $priority)
    {
        $fee = intdiv($amount * 35, 1000);
        if ($priority == 3) {
            $fee = $fee + 11;
        }
        return $fee;
    }
}

class LedgerAggregate
{
    public $count = 0;
    public $amountTotal = 0;
    public $feeTotal = 0;
    public $largeCount = 0;

    public function record($amount, $fee, $large)
    {
        $this->count = $this->count + 1;
        $this->amountTotal = $this->amountTotal + $amount;
        $this->feeTotal = $this->feeTotal + $fee;
        $this->largeCount = $this->largeCount + $large;
    }

    public function summary()
    {
        return [
            'count' => $this->count,
            'amount' => $this->amountTotal,
            'fees' => $this->feeTotal,
            'large' => $this->largeCount,
        ];
    }
}

function runLedgerPipeline($iterations)
{
    $policy = new LedgerFeePolicy();
    $ledger = new LedgerAggregate();

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
$result = runLedgerPipeline(500000);
$elapsed = microtime(true) - $start;
echo $result['count'] . ',' . $result['amount'] . ',' . $result['fees'] . ',' . $result['large'] . '|' . $elapsed;
