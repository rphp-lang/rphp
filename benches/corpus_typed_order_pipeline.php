<?php

// Typed counterpart of corpus_order_pipeline.php. The executed business flow
// is intentionally identical; only stable parameter and return contracts are
// declared so benchmark runs expose both benefits and boundary overhead.

class TypedQuoteRequest
{
    public $subtotal = 0;
    public $level = 0;
    public $region = '';

    public function __construct(int $subtotal, int $level, string $region)
    {
        $this->subtotal = $subtotal;
        $this->level = $level;
        $this->region = $region;
    }
}

class TypedDiscountPolicy
{
    public function rate(TypedQuoteRequest $request): int
    {
        $rate = 150;
        if ($request->level >= 3) {
            $rate = $rate + 250;
        }
        if ($request->subtotal >= 20000) {
            $rate = $rate + 175;
        }
        return $rate;
    }
}

class TypedTaxPolicy
{
    public function amount(int $net, string $region): int
    {
        if ($region == 'EU') {
            return intdiv($net * 2100, 10000);
        }
        if ($region == 'US') {
            return intdiv($net * 725, 10000);
        }
        return intdiv($net * 1200, 10000);
    }
}

class TypedQuoteService
{
    public $discountPolicy;
    public $taxPolicy;

    public function __construct(TypedDiscountPolicy $discountPolicy, TypedTaxPolicy $taxPolicy)
    {
        $this->discountPolicy = $discountPolicy;
        $this->taxPolicy = $taxPolicy;
    }

    public function quote(TypedQuoteRequest $request): array
    {
        $rate = $this->discountPolicy->rate($request);
        $discount = intdiv($request->subtotal * $rate, 10000);
        $net = $request->subtotal - $discount;
        $tax = $this->taxPolicy->amount($net, $request->region);
        return [
            'net' => $net,
            'tax' => $tax,
            'gross' => $net + $tax,
        ];
    }
}

function runTypedQuotePipeline(int $iterations): array
{
    $service = new TypedQuoteService(new TypedDiscountPolicy(), new TypedTaxPolicy());
    $netTotal = 0;
    $taxTotal = 0;
    $grossTotal = 0;
    $largeQuotes = 0;

    for ($i = 0; $i < $iterations; $i++) {
        if (($i % 3) == 0) {
            $region = 'EU';
        } elseif (($i % 3) == 1) {
            $region = 'US';
        } else {
            $region = 'ROW';
        }

        $subtotal = 5000 + (($i % 250) * 125);
        $request = new TypedQuoteRequest($subtotal, $i % 5, $region);
        $quote = $service->quote($request);
        $netTotal = $netTotal + $quote['net'];
        $taxTotal = $taxTotal + $quote['tax'];
        $grossTotal = $grossTotal + $quote['gross'];
        if ($quote['gross'] >= 25000) {
            $largeQuotes = $largeQuotes + 1;
        }
    }

    return [
        'net' => $netTotal,
        'tax' => $taxTotal,
        'gross' => $grossTotal,
        'large' => $largeQuotes,
    ];
}

$start = microtime(true);
$result = runTypedQuotePipeline(500000);
$elapsed = microtime(true) - $start;
echo $result['net'] . ',' . $result['tax'] . ',' . $result['gross'] . ',' . $result['large'] . '|' . $elapsed;
