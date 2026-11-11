#!/usr/bin/env python3
"""Build the checked-in Jade Beetle art into new, private item/monster content."""
import argparse
import json
import shutil
import struct
from pathlib import Path
from mcp_smoke import Client


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--assets',type=Path,required=True)
    parser.add_argument('--db',type=Path,required=True)
    parser.add_argument('--project',type=Path,required=True)
    args=parser.parse_args()
    args.binary=args.binary.resolve();args.assets=args.assets.resolve();args.db=args.db.resolve();args.project=args.project.resolve()
    args.project.mkdir()
    art=Path(__file__).resolve().parents[1]/'assets/authoring-demo'
    shutil.copyfile(art/'jade-beetle-talisman.png',args.project/'item.png')
    shutil.copyfile(art/'jade-beetle-atlas.png',args.project/'monster.png')
    with (args.project/'demo.stderr.log').open('w') as log:
        client=Client(args,log)
        try:
            client.rpc('initialize',{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'zircon-sprite-demo','version':'1'}})
            client.send('notifications/initialized',{})
            items=client.call('content_search',{'kind':'items','filter':'Ring'})['entries']
            item=next((r for r in items if r['name']=='CopperRing'),items[0])
            client.call('sprite_export_project',{'kind':'item','source_image':item['image'],'output':'item-source'})
            client.call('sprite_import_image',{'project':'project:item-source/project.json','image':'project:item.png','slot':0,'layer':'image','fit':True,'binary_alpha':True,'trim_transparent':True,'canvas':[32,32],'offsets':[-16,-16],'output':'item-art'})
            published_item=client.call('sprite_publish',{'project':'project:item-art/project.json','kind':'item','source_id':item['id'],'name':'Jade Beetle Ring','output':'item-content'})
            monsters=client.call('content_search',{'kind':'monsters','filter':'Beetle'})['entries']
            monster=next(r for r in monsters if r['image']==73)
            client.call('sprite_export_project',{'kind':'monster','source_image':73,'output':'monster-source'})
            # Explicit cel holds: alternating walk poses, windup/strike, hit and collapse.
            # Dead shares the final death slot; Skeleton uses the authored corpse pose.
            sequences=[(0,[0]*4),(80,[1,1,0,2,2,0]),(160,[3,3,4,4,0,0]),(240,[5,0]),(320,[6]*4+[7]*6),(880,[7])]
            mapping=[]
            for start,rows in sequences:
                for direction in range(8):
                    for frame,row in enumerate(rows):
                        mapping.append({'slot':start+direction*10+frame,'layer':'image','column':direction,'row':row})
            width,height=struct.unpack('>II',(args.project/'monster.png').read_bytes()[16:24])
            assert width==height and width//8>=128
            # Round up cell extent; the importer transparently pads partial edge cells.
            cell=(width+7)//8
            (args.project/'monster-mapping.json').write_text(json.dumps(mapping,indent=2)+'\n')
            imported=client.call('sprite_import_sheet',{'project':'project:monster-source/project.json','image':'project:monster.png','cell_width':cell,'cell_height':cell,'mapping':mapping,'fit':True,'binary_alpha':True,'canvas':[64,64],'offsets':[-32,-50],'replace_layers':True,'output':'monster-art'})
            assert imported['changed_slots']==len(mapping)
            client.call('sprite_validate_project',{'project':'project:monster-art/project.json'})
            client.call('sprite_preview_sheet',{'project':'project:monster-art/project.json','output':'monster-preview'})
            published_monster=client.call('sprite_publish',{'project':'project:monster-art/project.json','kind':'monster','source_id':monster['id'],'name':'Jade Beetle','database':'project:item-content/System.db','assets':'project:item-content/Client','output':'monster-content'})
            world=client.call('world_perimeter',{'rooms':2,'nw':3,'ne':4,'seed':84,'monster_id':published_monster['created_id'],'database':'project:monster-content/System.db','output':'world'})
            world.pop('spawn_cells',None)
            report={'status':'passed','item':published_item,'monster':published_monster,'world':world,'authored_frames':len(mapping),'assets':published_monster['assets'],'rpc_requests':client.next_id}
            (args.project/'demo-report.json').write_text(json.dumps(report,indent=2)+'\n')
            client.close()
            print(json.dumps(report,indent=2))
        finally:
            if client.process.poll() is None:
                client.process.kill();client.process.wait(timeout=10)

if __name__=='__main__':
    main()
