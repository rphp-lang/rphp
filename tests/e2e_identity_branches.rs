mod common;

use common::run_php;

#[test]
fn identity_branch_mixed() {
    assert_eq!(
        run_php(
            r###"<?php
function identityDecision($a, $b) { if ($a === $b) { return 1; } if ($a !== $b) { return 2; } return 3; }
$o=(object)['x'=>1]; $p=(object)['x'=>1]; $f=function(){};
$values=[null,false,true,0,1,-1,1.0,-0.0,NAN,INF,'','0','1',"\xff",[],[1],[1,2],['x'=>1],$o,$p,$o,$f,$f];
$out=''; foreach($values as $a) { foreach($values as $b) { $out .= identityDecision($a,$b); }}
echo hash('sha256',$out),"\n";
"###
        ),
        r###"5b57db07c31fd0cd1e70c033161e961878f3f85a576bf44cc9c7008f47a8b9b2
"###
    );
}

#[test]
fn identity_branch_references() {
    assert_eq!(
        run_php(
            r###"<?php
function aliasDecision(&$a, $b) { if ($a === $b) { $a=['next'=>2]; return 1; } if ($a !== null) { return 2; } return 3; }
$a=['next'=>1]; $alias=&$a; $b=$a;
echo aliasDecision($a,$b),':',json_encode($alias),':',json_encode($b),"\n";
function byValueDecision($a,$b) { if ($a !== $b) { return 'different'; } return 'same'; }
echo byValueDecision($alias,$b),':',byValueDecision($b,$b),"\n";
"###
        ),
        r###"1:{"next":2}:{"next":1}
different:same
"###
    );
}

#[test]
fn identity_branch_control() {
    assert_eq!(
        run_php(
            r###"<?php
function choose($a,$b,$c) { if (($a === $b && $b !== $c) || $a !== $c) { return 1; } return 0; }
$out='';foreach([null,0,1,'1',[],[1]] as $a){foreach([null,0,1,'1',[],[1]] as $b){foreach([null,0,1,'1',[],[1]] as $c){$out.=choose($a,$b,$c);}}}
echo hash('sha256',$out),"\n";
function loopDecision($n){$sum=0;for($i=0;$i<$n;$i++){if($i===500){$sum+=10;}elseif($i!==700){$sum+=1;}}return $sum;}
echo loopDecision(1000),"\n";
"###
        ),
        r###"7d851b8414a21360c8b75b4a0e09b35fc08c55ed51d95c583353cb7f36acbbea
1008
"###
    );
}

#[test]
fn identity_branch_lifetime() {
    assert_eq!(
        run_php(
            r###"<?php
class IdentityLifetime { public function __construct(public $id){} public function __destruct(){echo 'drop:',$this->id,"\n";} }
function observed($a,$b){ if($a===$b){echo "same\n";}else{echo "different\n";} }
observed(new IdentityLifetime(1),new IdentityLifetime(2));
function compareTemporary(){if(new IdentityLifetime(3)===new IdentityLifetime(4)){echo "yes\n";}else{echo "no\n";}}
compareTemporary();
set_error_handler(function($n,$m){echo "warning\n";return true;});
function undefDecision(){if($missing===null){echo "null\n";}else{echo "other\n";}} undefDecision();
"###
        ),
        r###"different
drop:1
drop:2
drop:3
drop:4
no
warning
null
"###
    );
}

#[test]
fn identity_branch_finally() {
    assert_eq!(
        run_php(
            r###"<?php
function finallyDecision($a,$b){try{if($a===$b){return 1;}return 2;}finally{echo "finally\n";}}
echo finallyDecision([1],[1]),':',finallyDecision(1,'1'),"\n";
function escapingDecision($a,$b){try{if($a!==$b){throw new RuntimeException('edge');}echo "same\n";}catch(RuntimeException $e){echo $e->getMessage(),"\n";}}
escapingDecision([1],[2]);escapingDecision(null,null);
"###
        ),
        r###"finally
1:finally
2
edge
same
"###
    );
}

#[test]
fn identity_branch_fiber() {
    assert_eq!(
        run_php(
            r###"<?php
function suspendedDecision($a,$b){if($a===$b){Fiber::suspend('same');}else{Fiber::suspend('different');}if($a!==null){return 2;}return 3;}
foreach([[1,1],[1,'1'],[null,null],[[],[]]] as $pair){$fiber=new Fiber(function()use($pair){return suspendedDecision($pair[0],$pair[1]);});echo $fiber->start(),':';$fiber->resume();echo $fiber->getReturn(),"\n";}
"###
        ),
        r###"same:2
different:2
same:3
same:2
"###
    );
}

#[test]
fn identity_branch_recursive_error_preserves_original_boundary() {
    assert_eq!(
        run_php(
            r###"<?php
function recursiveDecision($a, $b, $reverse) {
    if ($reverse) { if ($a !== $b) { return 'different'; } return 'same'; }
    if ($a === $b) { return 'same'; } return 'different';
}
$left = []; $left[0] =& $left;
$right = []; $right[0] =& $right;
foreach ([false, true] as $reverse) {
    try { echo recursiveDecision($left, $right, $reverse), "\n"; }
    catch (Error $error) { echo $error->getMessage(), "\n"; }
}
echo recursiveDecision($left, $left, false), "\n";
echo recursiveDecision($left, $left, true), "\n";
"###
        ),
        r###"Nesting level too deep - recursive dependency?
Nesting level too deep - recursive dependency?
same
same
"###
    );
}
