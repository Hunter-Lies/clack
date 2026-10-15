import re,sys,io
sys.stdout=io.TextIOWrapper(sys.stdout.buffer,encoding='utf-8',errors='replace')
for p in [r'E:\HUNTER\源码\运行库合集\tools\download\vc_redist_vs18_x64.exe', r'E:\HUNTER\源码\运行库合集\tools\download\vc_redist_vs17_x64.exe']:
    d=open(p,'rb').read()
    print('====',p.split('\\')[-1],len(d))
    for pat in [rb'Windows 7', rb'Windows 8', rb'Windows 10', rb'Windows 11', rb'6\.1\.7601', rb'10\.0\.\d{5}', rb'requires[^\x00]{0,80}']:
        m=re.findall(pat,d)
        if m:
            uniq=[]
            for x in m[:6]:
                s=x.decode('latin1') if isinstance(x,bytes) else x
                if s not in uniq: uniq.append(s)
            print('   %-18s %d 处  例: %s' % (pat.decode('latin1'),len(m),uniq[:4]))
