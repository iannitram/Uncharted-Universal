import re,sys,os,collections
funcs=open('/opt/homebrew/include/vulkan/vulkan_funcs.hpp').read().split('\n')
m2f=collections.defaultdict(set)   # method name -> vk funcs
cur=None
defre=re.compile(r'VULKAN_HPP_INLINE .*?\b(?:(\w+)::)?(\w+)\s*\(')
for l in funcs:
    d=re.match(r'\s*VULKAN_HPP_(?:INLINE|NODISCARD_WHEN_NO_EXCEPTIONS)\b.*?(?:(\w+)::)?(\w+)\s*\(',l)
    if d and ('::' in l or 'VULKAN_HPP_INLINE' in l) and not l.strip().startswith('//'):
        cur=d.group(2)
    for f in re.findall(r'\bd\.(vk\w+)\s*\(',l):
        if cur: m2f[cur].add(f)
# alias unique/raii names
root=sys.argv[1]
files=[]
for dp,dn,fn in os.walk(root):
    for f in fn:
        if f.endswith(('.cpp','.h','.mm','.hpp')): files.append(os.path.join(dp,f))
used=collections.defaultdict(set)
for f in files:
    try:t=open(f,errors='ignore').read()
    except: continue
    for mm in re.finditer(r'(?:\.|->|vk::|\b)(\w+)\s*\(',t):
        n=mm.group(1)
        if n in m2f:
            used[n].add(os.path.relpath(f,root))
        # unique variants
    for mm in re.finditer(r'(?:\.|->|vk::)(\w+?)Unique\s*\(',t):
        n=mm.group(1)
        if n in m2f: used[n].add(os.path.relpath(f,root))
# also direct vk* C names
direct=collections.defaultdict(set)
for f in files:
    t=open(f,errors='ignore').read()
    for n in set(re.findall(r'\b(vk[A-Z]\w+)\s*\(',t)): direct[n].add(os.path.relpath(f,root))
out=collections.defaultdict(set)
for n,fs in used.items():
    for vf in m2f[n]: out[vf]|=fs
for n,fs in direct.items(): out[n]|=fs
for k in sorted(out): print(k, len(out[k]))
