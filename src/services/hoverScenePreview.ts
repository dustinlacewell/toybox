import type { Box3, Group, WebGLRenderer } from "three";
import { Spherical, Vector3 } from "three";
import { createScenePreviewStage } from "./scenePreviewStage";
import { fitCameraToBounds } from "./fitCamera";
import { setPreviewLighting } from "./previewLighting";
import type { PreviewBrightness } from "../domain/previewBrightness";

/** Host preview extension: any scene source can supply playback and cleanup. */
export interface LiveScene {subject:Group;bounds:Box3;continuous?:boolean;prepare?(context:{renderer:WebGLRenderer}):void;rotate?(dx:number,dy:number):void;reset():void;tick(delta:number):void;dispose():void;}
export interface HoverPreviewSource {key:string;revision:readonly unknown[];load(signal:AbortSignal):Promise<LiveScene>;}
export type HoverPreviewState={kind:"loading"}|{kind:"ready"}|{kind:"error";message:string};
type Request={source:HoverPreviewSource;host:HTMLElement;controller:AbortController;scene?:LiveScene;notify:(state:HoverPreviewState)=>void;brightness:PreviewBrightness};
const same=(a:HoverPreviewSource,b:HoverPreviewSource)=>a.key===b.key&&a.revision.length===b.revision.length&&a.revision.every((value,i)=>value===b.revision[i]);

/** One canvas and frame loop for the library, with two recently used scenes. */
export class HoverScenePreview {
  private active?:Request;
  private stage?:ReturnType<typeof createScenePreviewStage>;
  private cache:{source:HoverPreviewSource;scene:LiveScene}[]=[];
  private frame=0;
  private pending?:ReturnType<typeof setTimeout>;
  private idle?:ReturnType<typeof setTimeout>;
  private observer?:ResizeObserver;
  private lastFrame=0;
  private visibility=()=>{if(document.hidden)this.stop();};
  private contextLost=(event:Event)=>{event.preventDefault();if(this.active)this.fail(this.active,Error("Graphics context lost"));this.destroyStage();};
  constructor(private options:{delayMs?:number;idleMs?:number}={}){}

  show(host:HTMLElement,source:HoverPreviewSource,notify:(state:HoverPreviewState)=>void):()=>void {
    this.stop();clearTimeout(this.idle);this.idle=undefined;
    const request:Request={host,source,notify,controller:new AbortController(),brightness:"normal"};this.active=request;
    document.addEventListener("visibilitychange",this.visibility);
    notify({kind:"loading"});
    // Avoid disk reads and retargeting while the pointer crosses other cards.
    this.pending=setTimeout(()=>{void this.load(request);},this.options.delayMs??120);
    return()=>{if(this.active===request)this.stop();};
  }

  private async load(request:Request) {
    try {
      const index=this.cache.findIndex(entry=>same(entry.source,request.source));
      const scene=index<0?await request.source.load(request.controller.signal):this.cache.splice(index,1)[0].scene;
      if(this.active!==request||request.controller.signal.aborted){scene.dispose();return;}
      request.scene=scene;
      if(!this.stage) {
        const canvas=document.createElement("canvas");canvas.setAttribute("aria-hidden","true");
        canvas.style.cssText="display:block;width:100%;height:100%;pointer-events:none";
        this.stage=createScenePreviewStage(canvas);
        canvas.addEventListener("webglcontextlost",this.contextLost);
        this.stage.renderer.setPixelRatio(Math.min(window.devicePixelRatio||1,1.5));
      }
      const stage=this.stage;stage.camera.zoom=1;scene.prepare?.({renderer:stage.renderer});request.host.appendChild(stage.renderer.domElement);stage.scene.add(scene.subject);scene.reset();
      setPreviewLighting(stage.scene,request.brightness);
      const resize=()=>{if(this.active!==request)return;const {width,height}=request.host.getBoundingClientRect();if(!width||!height)return;stage.renderer.setSize(Math.round(width),Math.round(height),false);stage.camera.aspect=width/height;fitCameraToBounds(stage.camera,scene.bounds);};
      resize();this.observer=new ResizeObserver(()=>{resize();if(this.active===request)stage.renderer.render(stage.scene,stage.camera);});this.observer.observe(request.host);
      stage.renderer.render(stage.scene,stage.camera);request.notify({kind:"ready"});
      this.lastFrame=performance.now();if(scene.continuous!==false)this.frame=requestAnimationFrame(this.tick);
    }catch(error) {
      if(this.active===request&&!request.controller.signal.aborted)this.fail(request,error);
    }
  }

  private tick=(now:number)=>{
    this.frame=0;
    const request=this.active,stage=this.stage;
    if(!request?.scene||!stage)return;
    if(!request.host.isConnected||document.hidden){this.stop();return;}
    try {if(request.scene.continuous===false||now-this.lastFrame>=1000/30){request.scene.tick(Math.min((now-this.lastFrame)/1000,.1));this.lastFrame=now;stage.renderer.render(stage.scene,stage.camera);}}
    catch(error){this.fail(request,error);return;}
    if(request.scene.continuous!==false)this.frame=requestAnimationFrame(this.tick);
  };

  rotate(host:HTMLElement|null,dx:number,dy:number) {
    const request=this.active;if(!host||request?.host!==host||!request.scene||!this.stage)return;
    if(request.scene.rotate)request.scene.rotate(dx,dy);
    else {
      // Fixed world-up orbit: pitch and yaw only, without rolling the subject.
      const camera=this.stage.camera,center=request.scene.bounds.getCenter(new Vector3());
      const spherical=new Spherical().setFromVector3(camera.position.clone().sub(center));
      spherical.theta-=dx*.01;spherical.phi=Math.max(.05,Math.min(Math.PI-.05,spherical.phi-dy*.01));
      camera.position.copy(center).add(new Vector3().setFromSpherical(spherical));camera.up.set(0,1,0);camera.lookAt(center);
    }
    if(!this.frame)this.frame=requestAnimationFrame(this.tick);
  }

  setBrightness(host:HTMLElement|null,brightness:PreviewBrightness) {
    const request=this.active;
    if(!host||request?.host!==host)return;
    request.brightness=brightness;
    if(this.stage&&request.scene) {
      setPreviewLighting(this.stage.scene,brightness);
      this.stage.renderer.render(this.stage.scene,this.stage.camera);
    }
  }

  zoom(host:HTMLElement|null,delta:number) {
    if(!host||this.active?.host!==host||!this.active.scene||!this.stage)return;
    const camera=this.stage.camera;
    camera.zoom=Math.max(.35,Math.min(4,camera.zoom*Math.exp(-delta*.01)));
    camera.updateProjectionMatrix();
    if(!this.frame)this.frame=requestAnimationFrame(this.tick);
  }

  stop() {
    clearTimeout(this.pending);this.pending=undefined;cancelAnimationFrame(this.frame);this.frame=0;
    document.removeEventListener("visibilitychange",this.visibility);this.observer?.disconnect();this.observer=undefined;
    const previous=this.active;this.active=undefined;
    if(previous){previous.controller.abort();if(previous.scene){this.stage?.scene.remove(previous.scene.subject);this.cache.push({source:previous.source,scene:previous.scene});while(this.cache.length>2)this.cache.shift()!.scene.dispose();}}
    this.stage?.renderer.domElement.remove();this.scheduleIdle();
  }

  private fail(request:Request,error:unknown) {
    this.active=undefined;request.controller.abort();cancelAnimationFrame(this.frame);this.frame=0;
    document.removeEventListener("visibilitychange",this.visibility);this.observer?.disconnect();this.observer=undefined;
    if(request.scene){this.stage?.scene.remove(request.scene.subject);request.scene.dispose();}
    this.stage?.renderer.domElement.remove();request.notify({kind:"error",message:String(error)});this.scheduleIdle();
  }
  private scheduleIdle(){clearTimeout(this.idle);this.idle=setTimeout(()=>this.dispose(),this.options.idleMs??15000);}
  private destroyStage(){const stage=this.stage;this.stage=undefined;if(stage){stage.renderer.domElement.removeEventListener("webglcontextlost",this.contextLost);stage.renderer.domElement.remove();stage.renderer.dispose();stage.renderer.forceContextLoss();}}
  dispose(){this.stop();clearTimeout(this.idle);this.cache.forEach(entry=>entry.scene.dispose());this.cache=[];this.destroyStage();}
}
export const hoverScenePreview=new HoverScenePreview();