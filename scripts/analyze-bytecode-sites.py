#!/usr/bin/env python3
"""Audit general block metadata in a canonical per-PC census; no runtime gate."""
from pathlib import Path
import argparse, collections, hashlib, json

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--input', required=True, type=Path)
parser.add_argument('--out-dir', required=True, type=Path)
args=parser.parse_args()
p=args.input.resolve();D=args.out_dir.resolve();D.mkdir(parents=True,exist_ok=True)
packet=json.loads(p.read_text())
assert packet['complete'] and packet['native_performance_acceptance'] is False
assert packet['instruction_field_order']==['opcode','op1_type','op2_type','result_type','op1','op2','result','flags','extended']
names={int(k):v for k,v in packet['opnames'].items()}
assert all(0<=k<256 and isinstance(v,str) for k,v in names.items())
assert len({r['id'] for r in packet['records']})==len(packet['records'])==packet['totals']['record_count']
decoded=collections.Counter()
for r in packet['records']:
    fields=r['instruction_fields'];sites=r['site_counts'];n=r['instructions']
    assert len(fields)==len(sites)==n and sum(sites)==r['decoded_steps']
    local_counts=collections.Counter()
    for i,count in zip(fields,sites):
        assert len(i)==9 and all(type(x) is int and x>=0 for x in i)
        assert type(count) is int and count>=0 and i[0] in names
        assert all(x<=4 for x in i[1:4]) and max(i[4:8])<=65535 and i[8]<=4294967295
        if count:local_counts[i[0]]+=count
    assert dict(local_counts)==dict(r['opcodes']);decoded.update(local_counts)
    assert all(len(f)==2 and 0<=f[0]<=11 and (f[1] is None or type(f[1]) is int) for f in r['literal_facts'])
    cursor=0
    for first,last in r['block_ranges']:
        assert first==cursor and first<=last<n;cursor=last+1
    assert not r['block_ranges'] or cursor==n
assert dict(decoded)==dict(packet['totals']['opcodes']) and sum(decoded.values())==packet['totals']['decoded_steps']
resume={'census_sha256':hashlib.sha256(p.read_bytes()).hexdigest()}

ARITHMETIC = {'Add', 'Sub', 'Mul', 'Mod', 'BitwiseAnd', 'BitwiseOr', 'BitwiseXor', 'ShiftLeft', 'ShiftRight'}
COMPARISON = {'IsEqual', 'IsNotEqual', 'IsSmaller', 'IsSmallerOrEqual', 'IsIdentical', 'IsNotIdentical'}
ALIASES = {'Add_CvConst': 'Add', 'Add_TmpTmp': 'Add', 'Sub_CvConst': 'Sub', 'Sub_TmpTmp': 'Sub',
           'IsSmaller_CvConst': 'IsSmaller', 'IsEqual_CvConst': 'IsEqual',
           'BitwiseAnd_LongLong': 'BitwiseAnd', 'BitwiseOr_LongLong': 'BitwiseOr', 'BitwiseXor_LongLong': 'BitwiseXor'}

def classify(record, fields, tier):
    opcode, a, b, result, x, y, z, flags, extended = fields
    name = ALIASES.get(names[opcode], names[opcode])
    total = record['cv_slots'] + record['tmp_slots']
    literals = record['literal_facts']
    slots = set()
    def slot(kind, index):
        if kind in [2, 3, 4] and index < total:
            slots.add(index)
            return True
        return False
    def operand(kind, index, primitive=False, key=False):
        if slot(kind, index):
            return True
        if kind != 1 or index >= len(literals):
            return False
        value_kind = literals[index][0]
        return value_kind == 4 or (primitive and value_kind in [0, 1, 2, 3]) or (key and value_kind == 6)
    def output():
        return slot(result, z)
    if name == 'ReleaseTemps':
        if a == b == 2 and result == 0 and flags in [0, 8] and x <= y <= total:
            slots.update(range(x, y))
            return 'release', slots
        return 'release_effect', slots
    if name == 'FetchCvR':
        if a == 4 and flags & ~5 == 0 and slot(a, x) and output():
            return 'scalar', slots
        return 'snapshot_effect', slots
    if name == 'AssignCv':
        if flags & ~2 == 0 and slot(a, x) and operand(b, y) and (result == 0 or output()):
            return 'scalar', slots
        return 'assignment_shape', slots
    if name in ARITHMETIC or name in COMPARISON:
        primitive = name in ['IsIdentical', 'IsNotIdentical']
        if operand(a, x, primitive) and operand(b, y, primitive) and output():
            return 'scalar', slots
        return 'non_long_literal_or_storage', slots
    if name in ['BoolNot', 'BitwiseNot']:
        if operand(a, x) and output():
            return 'scalar', slots
        return 'non_long_literal_or_storage', slots
    if name in ['PreInc', 'PreDec', 'PostInc', 'PostDec']:
        if slot(a, x) and (b == 0 or slot(b, y)) and (result == 0 or output()):
            return 'scalar', slots
        return 'incdec_shape', slots
    if name in ['JmpZ', 'JmpNZ']:
        if flags == 0 and operand(a, x):
            return 'branch', slots
        return 'branch_effect', slots
    if name == 'FetchDimR':
        if tier == 'scalar':
            return 'array_read_missing', slots
        if flags == 0 and slot(a, x) and operand(b, y, key=True) and result in [2, 3] and output():
            return 'array_read', slots
        return 'array_context_or_storage', slots
    if name == 'FetchObjR':
        if tier != 'property_array':
            return 'property_read_missing', slots
        if flags == 0 and slot(a, x) and b == 1 and y < len(literals) and literals[y][0] == 6 and result in [2, 3] and output():
            return 'property_read', slots
        return 'property_context_or_storage', slots
    return 'unsupported_' + name, slots

def segments(record, tier):
    fields = record['instruction_fields']
    spans = []
    rejected = collections.Counter()
    ranges = record['block_ranges'] or [[0, len(fields)-1]]
    for first, last in ranges:
        run = []
        def flush():
            nonlocal run
            if len(run) >= 4 and sum(kind != 'release' for _, kind, _ in run) >= 2:
                physical = set().union(*(slots for _, _, slots in run))
                if len(physical) <= 64:
                    spans.append(dict(first=run[0][0], last=run[-1][0],
                                      steps=sum(record['site_counts'][ip] for ip, _, _ in run),
                                      entry_hits=record['site_counts'][run[0][0]],
                                      sparse_slots=len(physical), physical_high=max(physical, default=0),
                                      kinds=dict(collections.Counter(kind for _, kind, _ in run))))
            run = []
        for ip in range(first, last + 1):
            kind, slots = classify(record, fields[ip], tier)
            if kind in ['scalar', 'release', 'array_read', 'property_read', 'branch']:
                run.append((ip, kind, slots))
                if kind == 'branch' or len(run) == 32:
                    flush()
            else:
                rejected[kind] += record['site_counts'][ip]
                flush()
        flush()
    return spans, rejected

def structural_tests():
    row = dict(cv_slots=2, tmp_slots=100, literal_facts=[[4, 1]],
               instruction_fields=[[11,4,1,2,0,0,100,0,0], [1,2,1,2,100,0,101,0,0],
                                   [10,4,2,0,1,101,0,0,0], [149,2,2,0,100,102,0,0,0]],
               site_counts=[3,3,3,3], block_ranges=[[0,3]])
    spans, rejected = segments(row, 'scalar')
    assert len(spans) == 1 and spans[0]['steps'] == 12 and spans[0]['sparse_slots'] == 4
    row['literal_facts'] = [[6, None]]
    assert not segments(row, 'scalar')[0]
    row['literal_facts'] = [[4, 1]]
    row['instruction_fields'][2][7] = 1
    assert not segments(row, 'scalar')[0]
    (D / 'audit-tests.json').write_text(json.dumps(dict(passed=True, sparse_wide_frame=True, nonlong_and_rebind_rejected=True), indent=2)+'\n')

structural_tests()
report = dict(complete=True, source_sha256=packet['source_sha256'], binary_sha256=packet['binary_sha256'],
              census_sha256=resume['census_sha256'], scope='execution-weighted structural envelope for offline general block design; not actual IR admission, guard success, native budget or measured improvement',
              native_performance_acceptance=False, parity_complete=False, tiers={},
              missing_proofs=['entry Value kinds and definedness', 'references/COW and array/object/lazy/magic/property visibility',
                              'temporary vacancy, borrowed-root lifetime, exact publication on every side exit',
                              'PHP-visible retirement/GC root effects, exceptions and original interrupt/GC boundaries',
                              'native instruction saving and code/stack/compile/memory bounds on both architectures'])
all_spans = {}
for tier in ['scalar', 'array', 'property_array']:
    rejected = collections.Counter()
    bodies = []
    spans_by_body = []
    for record in packet['records']:
        spans, misses = segments(record, tier)
        rejected.update(misses)
        if spans:
            bodies.append(dict(id=record['id'], name=record['name'], steps=sum(s['steps'] for s in spans),
                               span_count=len(spans), entry_hits=sum(s['entry_hits'] for s in spans),
                               wide_frame=record['cv_slots']+record['tmp_slots']>64))
            spans_by_body.append(dict(id=record['id'], spans=spans))
    total = sum(b['steps'] for b in bodies)
    report['tiers'][tier] = dict(potential_steps=total, percent_of_captured_steps=100*total/packet['totals']['decoded_steps'],
                               bodies=len(bodies), static_spans=sum(b['span_count'] for b in bodies),
                               potential_entry_hits=sum(b['entry_hits'] for b in bodies),
                               potential_steps_in_wide_frames=sum(b['steps'] for b in bodies if b['wide_frame']),
                               rejection_site_counts=dict(rejected), leading_bodies=sorted(bodies, key=lambda b:b['steps'], reverse=True)[:20])
    all_spans[tier] = spans_by_body
(D / 'structural-spans.json').write_text(json.dumps(all_spans, separators=(',', ':'))+'\n')
(D / 'structural-audit.json').write_text(json.dumps(report, indent=2)+'\n')
print('STRUCTURAL POTENTIAL', {tier: {k:v for k,v in row.items() if k not in ['rejection_site_counts','leading_bodies']} for tier,row in report['tiers'].items()}, flush=True)

records={r["id"]:r for r in packet["records"]}
spans=all_spans
def local_owner_envelope(record, span):
    first, last = span['first'], span['last']
    fields = record['instruction_fields']
    definitions = {}
    view_roots = set()
    views = set()
    retired = set()
    touched = set()
    for ip in range(first, last + 1):
        op, a, b, result, x, y, z, flags, extra = fields[ip]
        name = names[op]
        if name == 'ReleaseTemps':
            if any(slot not in definitions for slot in range(x, y)):
                return False, 'release_includes_pre_region_unknown_temporary'
            retired.update(range(x, y))
            continue
        if name in ['FetchObjR', 'FetchDimR']:
            if a in [2, 3]:
                if x not in definitions:
                    return False, 'heap_read_root_is_pre_region_temporary'
                views.add(x)
                if x in retired:
                    return False, 'read_view_already_retired'
            elif a == 4:
                if x in touched:
                    return False, 'heap_root_cv_was_assigned_in_region'
                view_roots.add(x)
            else:
                return False, 'unsupported_root_storage'
        if name == 'AssignCv':
            if a == 4:
                if x in view_roots:
                    return False, 'assignment_mutates_root_cv'
                touched.add(x)
            if a in [2, 3]:
                definitions[x] = ip
                retired.discard(x)
        if result in [2, 3]:
            definitions[z] = ip
            retired.discard(z)
    if not views <= retired:
        return False, 'borrowed_view_not_retired_inside_region'
    return True, 'local_metadata_owner_envelope'

def local_owner_tests():
    # Two ordinary property-array expressions, local scalar destinations and
    # their exact local release markers form one complete metadata envelope.
    row = dict(instruction_fields=[
        [101,4,1,2,0,0,3,0,0], [72,2,4,2,3,1,4,0,0],
        [149,2,2,0,3,4,0,8,0], [10,4,2,0,2,4,0,2,0],
        [149,2,2,0,3,5,0,0,0]])
    span = dict(first=0, last=4)
    assert local_owner_envelope(row, span)[0]
    assert not local_owner_envelope(row, dict(first=1,last=4))[0]
    row['instruction_fields'][3][4] = 0
    assert not local_owner_envelope(row, span)[0]
    row['instruction_fields'][3][4] = 2
    row['instruction_fields'][2][5] = 3
    row['instruction_fields'][4][5] = 3
    assert not local_owner_envelope(row, span)[0]

local_owner_tests()
report = dict(complete=True, source_sha256=packet['source_sha256'],
              census_sha256=hashlib.sha256(p.read_bytes()).hexdigest(),
              classifier_tests_passed=True, tiers={}, native_performance_acceptance=False, parity_complete=False,
              scope='Strict local metadata subset of prior structural spans: no unknown pre-region release owner, no external heap TMP root, root CV stable, intermediate read views locally retired. Not complete ownership/alias/type/GC/CFG proof or actual successful native coverage.',
              remaining_proofs=['dynamic exact Value kinds', 'source/result alias and reference/COW behavior',
                                'PHP property visibility, lazy/hook/magic and array-key effects',
                                'temporary overwrite vacancy and all side-exit publications',
                                'GC candidate/poll and interrupt boundaries', 'native instruction/architecture gates'])
retained = {}
for tier, rows in spans.items():
    rejects = collections.Counter()
    admitted = []
    bodies = []
    for row in rows:
        record = records[row['id']]
        selected = []
        for span in row['spans']:
            passed, reason = local_owner_envelope(record, span)
            if passed:
                selected.append(span)
            else:
                rejects[reason] += span['steps']
        if selected:
            admitted.append(dict(id=row['id'], spans=selected))
            bodies.append(dict(id=row['id'], name=record['name'],
                               steps=sum(span['steps'] for span in selected),
                               entry_hits=sum(span['entry_hits'] for span in selected),
                               spans=len(selected)))
    total = sum(body['steps'] for body in bodies)
    report['tiers'][tier] = dict(local_metadata_steps=total, percent_of_captured_steps=100*total/packet['totals']['decoded_steps'],
                               bodies=len(bodies), static_spans=sum(body['spans'] for body in bodies),
                               potential_entry_hits=sum(body['entry_hits'] for body in bodies),
                               rejection_step_counts=dict(rejects), leading_bodies=sorted(bodies,key=lambda b:b['steps'],reverse=True)[:20])
    retained[tier] = admitted
(D/'local-owner-spans.json').write_text(json.dumps(retained,separators=(',',':'))+'\n')
(D/'local-owner-audit.json').write_text(json.dumps(report,indent=2)+'\n')
print('LOCAL METADATA ENVELOPE', {tier:{k:v for k,v in row.items() if k not in ['rejection_step_counts','leading_bodies']} for tier,row in report['tiers'].items()},flush=True)
