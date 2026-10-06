<?php
function scalarWork(int $value = 4): int { return $value + strlen('abc'); }
function arrayWork(array $values): int { return count($values); }
class ScopeParent {
    static function enter(): static {
        $pad0 = 0;
        $pad1 = 1;
        $pad2 = 2;
        $pad3 = 3;
        $pad4 = 4;
        $pad5 = 5;
        $pad6 = 6;
        $pad7 = 7;
        $pad8 = 8;
        $pad9 = 9;
        $pad10 = 10;
        $pad11 = 11;
        $pad12 = 12;
        $pad13 = 13;
        $pad14 = 14;
        $pad15 = 15;
        $pad16 = 16;
        $pad17 = 17;
        $pad18 = 18;
        $pad19 = 19;
        $pad20 = 20;
        $pad21 = 21;
        $pad22 = 22;
        $pad23 = 23;
        $pad24 = 24;
        $pad25 = 25;
        $pad26 = 26;
        $pad27 = 27;
        $pad28 = 28;
        $pad29 = 29;
        $pad30 = 30;
        $pad31 = 31;
        $pad32 = 32;
        $pad33 = 33;
        $pad34 = 34;
        $pad35 = 35;
        $pad36 = 36;
        $pad37 = 37;
        $pad38 = 38;
        $pad39 = 39;
        $pad40 = 40;
        $pad41 = 41;
        $pad42 = 42;
        $pad43 = 43;
        $pad44 = 44;
        $pad45 = 45;
        $pad46 = 46;
        $pad47 = 47;
        $pad48 = 48;
        $pad49 = 49;
        $pad50 = 50;
        $pad51 = 51;
        $pad52 = 52;
        $pad53 = 53;
        $pad54 = 54;
        $pad55 = 55;
        $pad56 = 56;
        $pad57 = 57;
        $pad58 = 58;
        $pad59 = 59;
        $pad60 = 60;
        $pad61 = 61;
        $pad62 = 62;
        $pad63 = 63;
        $pad64 = 64;
        $pad65 = 65;
        $pad66 = 66;
        $pad67 = 67;
        $pad68 = 68;
        $pad69 = 69;
        $pad70 = 70;
        $pad71 = 71;
        echo static::class, ':', scalarWork(), ':', arrayWork([1, 2]), '|';
        try { scalarWork([]); } catch (TypeError $error) {
            echo 'type:', $error->getTrace()[0]['function'], '|';
        }
        $callback = fn () => static::class;
        echo $callback(), ':', get_called_class(), '|';
        return new static;
    }
    function ownType(self $value): self { return $value; }
}
class ScopeChild extends ScopeParent {}
$owner = ScopeChild::enter();
echo get_class($owner), ':', get_class($owner->ownType($owner)), '|';
echo get_class(ScopeParent::enter()), '|done';
