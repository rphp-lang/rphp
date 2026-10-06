"""Read native positions emitted by Callgrind's dump.c writer.

Association source/target positions use the last ordinary self-cost position.
They do not update that compression base. Treating a call-cost source as the
new base silently shifts later addresses when that source is not ``*``.
The exact Valgrind 3.22.0 fprint_fcost/fprint_jcc implementation establishes
this behavior; the general format manual does not spell out this distinction.
"""

from collections import Counter
from dataclasses import dataclass
from typing import Iterable
import re


@dataclass(frozen=True)
class Record:
    kind: str
    function: str
    position: tuple[int, ...]
    costs: tuple[int, ...]
    object: str = ""
    target_function: str = ""
    target_position: tuple[int, ...] = ()
    target_object: str = ""
    count: int = 0
    executions: int = 0


@dataclass
class Profile:
    positions: tuple[str, ...]
    events: tuple[str, ...]
    records: list[Record]
    totals: tuple[int, ...]

    def self_costs(self, event: str = "Ir") -> Counter:
        index = self.events.index(event)
        result = Counter()
        for record in self.records:
            if record.kind == "self":
                result[record.object, record.function, record.position] += record.costs[index]
        return result


def read_profile(lines: Iterable[str]) -> Profile:
    positions = ("line",)
    events: tuple[str, ...] = ()
    base = (0,)
    names: dict[int, str] = {}
    objects: dict[int, str] = {}
    function = call_target = jump_target = ""
    object_name = call_object = jump_object = ""
    pending = None
    records = []
    totals = None
    part_seen = False

    def decode(fields):
        if len(fields) != len(base):
            raise ValueError("wrong number of position fields")
        return tuple(
            old if token == "*" else old + int(token) if token[0] in "+-"
            else int(token, 16) if token.startswith("0x") else int(token)
            for old, token in zip(base, fields)
        )

    for number, raw in enumerate(lines, 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("part:"):
            if part_seen:
                raise ValueError("multiple parts require separate readers")
            part_seen = True
        elif line.startswith("positions:"):
            positions = tuple(line.split()[1:])
            base = (0,) * len(positions)
        elif line.startswith("events:"):
            events = tuple(line.split()[1:])
        elif line.startswith("totals:"):
            totals = tuple(map(int, line.split()[1:]))
        elif line.startswith(("fn=", "cfn=", "jfn=", "ob=", "cob=", "job=")):
            key, value = line.split("=", 1)
            table = names if key.endswith("fn") else objects
            match = re.fullmatch(r"\((\d+)\)(?:\s+(.*))?", value)
            if match:
                ident = int(match[1])
                if match[2] is not None:
                    table[ident] = match[2]
                value = table[ident]
            if key == "fn":
                function = value
            elif key == "cfn":
                call_target = value
            elif key == "jfn":
                jump_target = value
            elif key == "ob":
                object_name = value
            elif key == "cob":
                call_object = value
            else:
                jump_object = value
        elif line.startswith(("calls=", "jump=", "jcnd=")):
            if pending is not None:
                raise ValueError(f"missing association source at line {number}")
            kind, tail = line.split("=", 1)
            count, *fields = tail.split()
            if kind == "jcnd":
                taken, executions = map(int, count.split("/"))
            else:
                taken = executions = int(count)
            pending = (
                "call" if kind == "calls" else "jump",
                call_target if kind == "calls" else jump_target or function,
                decode(fields),
                taken,
                executions,
                (call_object if kind == "calls" else jump_object) or object_name,
            )
        elif re.match(r"^(?:0x[0-9a-fA-F]+|[+-]?\d+|\*)(?:\s|$)", line):
            fields = line.split()
            source = decode(fields[:len(base)])
            costs = tuple(map(int, fields[len(base):]))
            if not events or len(costs) > len(events):
                raise ValueError(f"invalid event fields at line {number}")
            costs += (0,) * (len(events) - len(costs))
            if pending is None:
                records.append(Record("self", function, source, costs, object=object_name))
                base = source
            else:
                kind, target, destination, count, executions, target_object = pending
                records.append(Record(
                    kind, function, source, costs, object=object_name,
                    target_function=target, target_position=destination,
                    target_object=target_object, count=count, executions=executions,
                ))
                # dump.c publishes target metadata separately for each edge.
                call_object = jump_object = jump_target = ""
                pending = None
        # File/object/debug metadata and summaries do not change positions.

    if pending is not None:
        raise ValueError("missing final association source")
    if not events:
        raise ValueError("missing events header")
    computed = tuple(
        sum(record.costs[i] for record in records if record.kind == "self")
        for i in range(len(events))
    )
    if totals is not None and totals != computed:
        raise ValueError(f"self totals disagree: {computed} != {totals}")
    return Profile(positions, events, records, computed)
