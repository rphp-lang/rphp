<?php
error_reporting(E_ALL);
set_error_handler(function ($level, $message) { echo "diagnostic:$level:$message\n"; return true; });
class SlotView extends SplFixedArray { public $caption = 'label'; }
$slots = new SlotView(3);
$slots[0] = 'north';
$slots[2] = ['east' => 4];
$slots->{'0'} = 'member';
echo "views\n";
var_dump($slots->toArray(), (array) $slots, $slots->__serialize());
echo "members-only\n";
var_dump(get_mangled_object_vars($slots));
echo "raw-debug\n";
var_dump($slots);
$encoded = serialize($slots);
echo $encoded, "\n";
$roundtrip = unserialize($encoded);
var_dump($roundtrip->toArray(), (array) $roundtrip, $roundtrip->caption);
$roundtrip->setSize(4);
$roundtrip[3] = 'last';
var_dump($roundtrip->toArray(), $slots->toArray());
echo "cycle\n";
$cycle = new SplFixedArray(1);
$cycle[0] = $cycle;
$encoded = serialize($cycle);
echo $encoded, "\n";
$restored = unserialize($encoded);
var_dump($restored[0] === $restored, $restored->getSize());
