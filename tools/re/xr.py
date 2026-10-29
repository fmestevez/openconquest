"""Disassembly and data lookups on ancient.exe (from the objdump listing).

As a library:  sys.path.insert(0, 'tools/re'); from xr import *
From the shell:
  xr.py show ADDR [N]       N instructions from ADDR
  xr.py dis START END       a range, with string constants noted
  xr.py refs VALUE          instructions mentioning an address or constant
  xr.py func ADDR           start of the function holding ADDR
  xr.py str TEXT            where a string sits
  xr.py data VALUE          dwords equal to VALUE in the data (vtables...)
  xr.py words ADDR N        N little-endian shorts at ADDR (tables)
  xr.py dwords ADDR N       N dwords at ADDR
VA = file offset + 0x400000 for every section of this executable.
"""
import re,struct,bisect,sys,pickle,os
RE=os.environ.get('OPENCONQUEST_RE', os.path.expanduser('~/Library/Caches/openconquest-re'))
S=RE
EXE=os.environ.get('OPENCONQUEST_EXE', os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'gamedata', 'ancient.exe'))
D=open(EXE,'rb').read()
def va2fo(va):
    if 0x401000<=va<0x594000: return va-0x400000
    if 0x594000<=va<0x5a2000: return va-0x400000
    if 0x5a2000<=va<0x5c7000: return va-0x400000
    return None
def rd(va,n): fo=va2fo(va); return D[fo:fo+n]
def u32(va): return struct.unpack('<I',rd(va,4))[0]
def cstr(va): b=rd(va,200); return b.split(b'\0')[0].decode('latin1')
pk=S+'/ac.pkl'
if os.path.exists(pk):
    ADDR,TXT=pickle.load(open(pk,'rb'))
else:
    ADDR=[];TXT=[]
    for line in open(S+'/ac.asm'):
        m=re.match(r'\s*([0-9a-f]+):\s+(.*)',line)
        if m: ADDR.append(int(m.group(1),16)); TXT.append(m.group(2).strip())
    pickle.dump((ADDR,TXT),open(pk,'wb'))
def idx(a): return bisect.bisect_left(ADDR,a)
def refs(val):
    h=hex(val); return [ADDR[i] for i,t in enumerate(TXT) if h in t and re.search(re.escape(h)+r'\b',t)]
def show(a,n=30,before=0):
    i=idx(a)-before
    for j in range(i,i+n): print(hex(ADDR[j]),TXT[j])
def func_start(a):
    i=idx(a)
    while i>0:
        if TXT[i].startswith('push\tebp') and TXT[i+1].startswith('mov\tebp, esp'): return ADDR[i]
        if TXT[i-1].startswith('int3') or TXT[i-1].startswith('ret') or TXT[i-1]=='nop': 
            if not TXT[i].startswith('nop') and not TXT[i].startswith('int3'): return ADDR[i]
        i-=1
def find_str(s):
    b=s.encode('latin1'); out=[]; i=D.find(b)
    while i>=0: out.append(i+0x400000); i=D.find(b,i+1)
    return out
def data_refs(val):
    "dword occurrences of val in data sections"
    b=struct.pack('<I',val); out=[]; i=D.find(b)
    while i>=0: out.append(i+0x400000); i=D.find(b,i+1)
    return out
def fstart(a):
    i=idx(a)
    while i>0:
        if TXT[i].startswith('push\tebp') and TXT[i+1].startswith('mov\tebp, esp'): return ADDR[i]
        i-=1
def vtable_of(fn):
    "data locations pointing to fn"
    return data_refs(fn)
import collections
_IMM=None
def imm_index():
    global _IMM
    if _IMM is None:
        _IMM=collections.defaultdict(list)
        for i,t in enumerate(TXT):
            for m in re.finditer(r'0x[0-9a-f]{6}\b',t):
                _IMM[int(m.group(0),16)].append(i)
    return _IMM
def vt_writes(slot):
    I=imm_index(); out=[]
    for v in range(slot-0x80, slot+4, 4):
        for i in I.get(v,[]):
            if TXT[i].startswith('mov\tdword ptr [') and TXT[i].endswith(hex(v)): out.append((v,ADDR[i]))
    return out
def dis(a,b):
    i=idx(a)
    while i<len(ADDR) and ADDR[i]<b:
        t=TXT[i]; extra=''
        for m in re.finditer(r'0x5[9a-c][0-9a-f]{4}\b',t):
            v=int(m.group(0),16)
            try:
                s=cstr(v)
                if len(s)>=3 and all(32<=ord(c)<127 for c in s): extra+='   ; '+repr(s)[:50]
            except Exception: pass
        print(hex(ADDR[i]),t,extra); i+=1

if __name__ == '__main__':
    a = sys.argv[1:]
    n = lambda s: int(s, 0)
    if not a:
        print(__doc__)
    elif a[0] == 'show':
        show(n(a[1]), n(a[2]) if len(a) > 2 else 30)
    elif a[0] == 'dis':
        dis(n(a[1]), n(a[2]))
    elif a[0] == 'refs':
        for r in refs(n(a[1])): print(hex(r), TXT[idx(r)])
    elif a[0] == 'func':
        print(hex(func_start(n(a[1]))))
    elif a[0] == 'str':
        print([hex(x) for x in find_str(a[1])])
    elif a[0] == 'data':
        print([hex(x) for x in data_refs(n(a[1]))])
    elif a[0] == 'words':
        print(list(struct.unpack('<%dh' % n(a[2]), rd(n(a[1]), 2 * n(a[2])))))
    elif a[0] == 'dwords':
        print([hex(x) for x in struct.unpack('<%dI' % n(a[2]), rd(n(a[1]), 4 * n(a[2])))])
