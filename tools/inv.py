import os,sys,io
sys.stdout=io.TextIOWrapper(sys.stdout.buffer,encoding='utf-8',errors='replace')
root=r'E:\HUNTER\源码\运行库合集\_src\2020'
def ver(p):
    try:
        import win32api
    except Exception:
        pass
    return None
# 用 PowerShell 拿版本太重，这里只按文件名+大小列出结构
print('=== 根目录结构 ===')
for f in sorted(os.listdir(root)):
    p=os.path.join(root,f)
    print('  %s%s' % (f, '/' if os.path.isdir(p) else '  %d' % os.path.getsize(p)))
print()
print('=== tmp\\ 各段里的安装器 (exe/msi) ===')
for sec in sorted(os.listdir(os.path.join(root,'tmp'))):
    d=os.path.join(root,'tmp',sec)
    if not os.path.isdir(d): continue
    items=[f for f in sorted(os.listdir(d)) if os.path.isfile(os.path.join(d,f)) and f.lower().endswith(('.exe','.msi','.msu'))]
    if items:
        print('  [%s] %s' % (sec, ', '.join('%s(%dK)'%(f,os.path.getsize(os.path.join(d,f))//1024) for f in items)))
print()
print('=== tmp\\ 各段的 System/System64 关键 dll ===')
keys=('msvcp','msvcr','msvbvm','concrt','vcruntime','mfc140','vccorlib','ucrtbase')
for sec in sorted(os.listdir(os.path.join(root,'tmp'))):
    d=os.path.join(root,'tmp',sec)
    if not os.path.isdir(d): continue
    for sub in ('System','System64'):
        sd=os.path.join(d,sub)
        if not os.path.isdir(sd): continue
        hits=[f for f in sorted(os.listdir(sd)) if any(k in f.lower() for k in keys)]
        if hits: print('  [%s\\%s] %s' % (sec,sub,', '.join(hits[:12])))
print()
print('=== sys\\ 里的老运行库 ===')
sd=os.path.join(root,'sys')
if os.path.isdir(sd):
    for f in sorted(os.listdir(sd)):
        print('  %-22s %8d' % (f,os.path.getsize(os.path.join(sd,f))))
