<?php
$data = '{"name":"Alice","age":30,"scores":[95,87,92],"address":{"city":"Prague","zip":"11000"}}';
$sum = 0;
for ($i = 0; $i < 10000; $i++) {
    $obj = json_decode($data, true);
    $sum += $obj["age"] + $obj["scores"][0] + $obj["scores"][1] + $obj["scores"][2];
}
echo $sum;
