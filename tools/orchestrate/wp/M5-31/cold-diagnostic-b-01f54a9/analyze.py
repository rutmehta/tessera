import json,re
from pathlib import Path
out=Path(__file__).parent
results={}
for fixture in ['original','unique']:
 text=(out/(fixture+'-1.log')).read_text()
 events=[]
 for line in text.splitlines():
  m=re.search(r'COLD-SPAN thread=ThreadId\((\d+)\) start_us=(\d+) end_us=(\d+) duration_us=(\d+) label=(.*)',line)
  if m:
   thread,start,end,duration,label=m.groups();events.append(dict(thread=int(thread),start=int(start),end=int(end),duration=int(duration),label=label))
 cold=next(e for e in events if e['label']=='MEASURED_COLD')
 cold_events=[e for e in events if e['start']>=cold['start'] and e['end']<=cold['end']]
 worker_overlap=[e for e in events if e['thread']!=cold['thread'] and e['end']>cold['start'] and e['start']<cold['end']]
 result=dict(cold=cold,cold_events=cold_events,worker_overlap=worker_overlap,all_events=events)
 results[fixture]=result
 print(fixture,'cold_ms',cold['duration']/1000)
 for e in cold_events:
  if e['thread']==cold['thread'] and (e['duration']>=1000 or e['label']=='render.final_wait'):
   print(' ',e['label'],round((e['start']-cold['start'])/1000,3),round(e['duration']/1000,3))
 print('workers',[(e['label'],round((e['start']-cold['start'])/1000,3),round(e['duration']/1000,3)) for e in worker_overlap])
(out/'spans.json').write_text(json.dumps(results,indent=2)+'\n')
