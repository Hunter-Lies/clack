import os,sys,io,subprocess,json
sys.stdout=io.TextIOWrapper(sys.stdout.buffer,encoding='utf-8',errors='replace')
root=r'E:\HUNTER\源码\运行库合集\_src\2020\tmp'
for sec in sorted(os.listdir(root)):
    d=os.path.join(root,sec)
    if not os.path.isdir(d): continue
    files=[f for f in sorted(os.listdir(d)) if os.path.isfile(os.path.join(d,f)) and f.lower().endswith(('.exe','.msi','.msu'))]
    for f in files:
        p=os.path.join(d,f)
        print('%s|%s|%d' % (sec,f,os.path.getsize(p)))
