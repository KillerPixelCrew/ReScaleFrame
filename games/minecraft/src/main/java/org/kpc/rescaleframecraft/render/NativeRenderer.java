package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import java.nio.FloatBuffer;
import java.util.function.Supplier;
import net.minecraft.client.Minecraft;
import org.joml.Matrix4f;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.client.TaaSettings;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.BufferUtils;
import org.lwjgl.glfw.GLFWNativeWin32;
import org.lwjgl.opengl.*;

/** Game ownership, render resolution, input conventions and HUD boundaries live here. */
public final class NativeRenderer {
    private static long handle,surfaceWindow,frame,closedFrame,lastFrameTime;
    private static final CpuFrameFacts cpuFrames=new CpuFrameFacts((source,stage)->NativeBridge.cpu(handle,source,stage));
    private static Configuration applied,failed;
    private static String status="native processing off";
    private static int[] surfaces;
    private static final long[] nativeStatus=new long[NativeStatus.COUNT];
    private static boolean nativeStatusAvailable;
    private static RenderTarget display;
    private static WorldRenderTarget world;
    private static boolean worldRendering;
    private static GlProgram guides,copyDepth;
    private static int framebuffer,readFramebuffer,vao;
    private static int displayDepth,frameDisplayWidth,frameDisplayHeight;
    private static int acceptedWidth,lastResult,refusedFrames;
    private static final FloatBuffer cameraData=BufferUtils.createFloatBuffer(88);
    private static final int[] GUIDE_ATTACHMENTS={GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1,GL30.GL_COLOR_ATTACHMENT2};
    private static final Matrix4f remap=new Matrix4f().m11(-1).m22(.5f).m32(.5f);
    private static final Matrix4f inverseRemap=new Matrix4f(remap).invert();
    private static final Matrix4f projection=new Matrix4f(),inverseProjection=new Matrix4f();
    private static final Matrix4f viewToWorld=new Matrix4f(),worldToView=new Matrix4f();
    private static final Matrix4f currentInverse=new Matrix4f(),previousClip=new Matrix4f(),depthRemap=new Matrix4f();
    private static float frameTime=.016f;
    private static final InputFrames inputs=new InputFrames((id,kinds)->NativeBridge.input(handle,id,kinds));
    private record Configuration(int width,int height,long revision) {
        boolean matches(int width,int height,long revision) {
            return this.width==width&&this.height==height&&this.revision==revision;
        }
    }
    private NativeRenderer() {}
    public static void beginFrame() {
        inputs.end();
        var game=Minecraft.getInstance();var s=TaaSettings.current;
        long now=System.nanoTime();
        frameTime=lastFrameTime==0?.016f:Math.clamp((now-lastFrameTime)*1e-9f,.001f,1f);lastFrameTime=now;
        if(handle!=0&&frame>closedFrame)abortFrame();
        ++frame;worldRendering=false;nativeStatusAvailable=false;
        if(!GL.getCapabilities().OpenGL33)return;
        display=game.getMainRenderTarget();
        frameDisplayWidth=display.viewWidth;frameDisplayHeight=display.viewHeight;
        configure(game,s);
        if(handle!=0&&surfaces!=null) {
            cpuFrames.simulation(frame);
            inputs.begin(frame);
            nativeStatusAvailable=NativeBridge.statusCount(handle,nativeStatus)!=0;
            if(!nativeStatusAvailable) {
                status="native status unavailable";
                return;
            }
            status=null;
        }
    }
    private static void configure(Minecraft game,TaaSettings s) {
        long revision=handle==0?0:NativeBridge.settingsRevision(handle);
        if((applied==null||!applied.matches(display.viewWidth,display.viewHeight,revision))&&
           (failed==null||!failed.matches(display.viewWidth,display.viewHeight,revision))) {
            var configuration=new Configuration(display.viewWidth,display.viewHeight,revision);
            try(var saved=new GlState(true)) {
                if(handle==0) {
                    var root=NativeBridge.load();
                    long parent=GLFWNativeWin32.glfwGetWin32Window(game.getWindow().getWindow());
                    if(surfaceWindow==0)surfaceWindow=windowThread(()->NativeBridge.createSurface(parent));
                    if(surfaceWindow==0)throw new IllegalStateException("Presentation surface must be created on the GLFW event thread");
                    handle=NativeBridge.open(parent,surfaceWindow,root.toString());
                    if(handle==0)throw new IllegalStateException("Native device/presentation initialization refused; see minecraft-native.log");
                }
                if(revision==0) {
                    var path=TaaSettings.nativeSettingsPath();
                    java.nio.file.Files.createDirectories(path.getParent());
                    int result=NativeBridge.openSettings(handle,path.toString(),s.upscaler.id,s.quality.ordinal(),
                        s.dlssPreset.id,s.frameGeneration.id,s.generatedFrames,s.reflex.ordinal(),s.frameLimit,s.nativeDebug,s.sharpening());
                    if(result!=0) {
                        if(result==NativeBridge.BACKEND_NOT_READY&&NativeBridge.close(handle)==0)handle=0;
                        throw new IllegalStateException("Shared settings initialization refused: "+result);
                    }
                    configuration=new Configuration(display.viewWidth,display.viewHeight,NativeBridge.settingsRevision(handle));
                }
                if(guides==null)guides=new GlProgram("fullscreen.vert","native-guides.frag");
                if(copyDepth==null)copyDepth=new GlProgram("fullscreen.vert","copy-depth.frag");
                if(framebuffer==0) {
                    framebuffer=GL30.glGenFramebuffers();readFramebuffer=GL30.glGenFramebuffers();vao=GL30.glGenVertexArrays();
                }
                var replacement=NativeBridge.configureSettings(handle,display.viewWidth,display.viewHeight);
                if(replacement==null)throw new IllegalStateException("Native reconfiguration failed; see minecraft-native.log");
                boolean resize=surfaces==null||surfaces[0]!=replacement[0]||surfaces[1]!=replacement[1];surfaces=replacement;
                if(resize) {
                    if(world==null)world=new WorldRenderTarget(surfaces[0],surfaces[1],Minecraft.ON_OSX);
                    else world.resize(surfaces[0],surfaces[1],Minecraft.ON_OSX);
                }
                applied=configuration;failed=null;acceptedWidth=0;TemporalRenderer.reset();
                ReScaleFrameCraft.LOGGER.info("ReScaleFrame native settings applied: {}, world {}x{}, display {}x{}",configuration,surfaces[0],surfaces[1],surfaces[2],surfaces[3]);
            } catch(Exception|LinkageError error) {
                failed=configuration;status=error.getMessage();ReScaleFrameCraft.LOGGER.error("ReScaleFrame native configuration failed",error);
            }
        }
    }
    private static <T> T windowThread(Supplier<T> work) throws Exception {
        // Optional public Ixeris API. Only window creation crosses this boundary;
        // GPU resources, device calls and presentation stay on the render thread.
        Class<?> type;
        try {type=Class.forName("me.decce.ixeris.api.IxerisApi");}
        catch(ClassNotFoundException absent) {return work.get();}
        Object api=type.getMethod("getInstance").invoke(null);
        @SuppressWarnings("unchecked") T result=(T)type.getMethod("query",Supplier.class).invoke(api,work);
        return result;
    }
    private static String name(long id) {return switch((int)id) {case 0->"Off";case 1->"DLSS";case 2->"FSR2";case 3->"FSR3";case 4->"FSR4";case 5->"XeSS";case 7->"Auto";default->"Unknown";};}
    public static void input(int kinds) {if(handle!=0)inputs.sample(kinds);}
    public static void simulationEnd() {inputs.end();if(handle!=0)NativeBridge.cpu(handle,frame,NativeBridge.SIMULATION_END);}
    static boolean nativeRequested() {
        return nativeStatusAvailable&&(NativeStatus.requestedSr(nativeStatus)!=0||NativeStatus.requestedFg(nativeStatus)!=0);
    }
    public static void beginWorld() {
        if(!nativeRequested()||handle==0||surfaces==null||world==null||closedFrame==frame)return;
        worldRendering=true;world.setClearColor(0,0,0,0);world.clear(Minecraft.ON_OSX);world.bindWrite(true);
    }
    public static RenderTarget worldTarget() {return worldRendering?world:null;}
    public static boolean processing() {return worldRendering&&surfaces!=null;}
    public static boolean upscaling() {return processing()&&nativeStatusAvailable&&NativeStatus.effectiveSr(nativeStatus)!=0;}
    public static int phaseCount() {return upscaling()?Math.max(8,Math.round(8f*surfaces[2]*surfaces[3]/surfaces[0]/surfaces[1])):8;}
    static void finishWorld(RenderTarget source,TemporalCamera camera,int depth,int motion) {
        if(!processing())return;
        // Three shared MRTs carry scene colour, canonical depth and dense unjittered motion.
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,framebuffer);
        for(int i=0;i<3;i++)GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0+i,GL11.GL_TEXTURE_2D,surfaces[4+i],0);
        GL20.glDrawBuffers(GUIDE_ATTACHMENTS);
        if(GL30.glCheckFramebufferStatus(GL30.GL_FRAMEBUFFER)!=GL30.GL_FRAMEBUFFER_COMPLETE)throw new IllegalStateException("Native guide framebuffer is incomplete");
        RenderSystem.viewport(0,0,surfaces[0],surfaces[1]);
        if(GL.getCapabilities().OpenGL45||GL.getCapabilities().GL_ARB_clip_control)GL45.glClipControl(GL45.GL_LOWER_LEFT,GL45.GL_NEGATIVE_ONE_TO_ONE);
        guides.use();guides.integer("sceneColor",0);guides.integer("sceneDepth",1);guides.integer("sceneMotion",2);
        TextureUnits.bind(0,source.getColorTextureId());TextureUnits.bind(1,depth);TextureUnits.bind(2,motion);
        for(int i=0;i<3;i++)GL33.glBindSampler(i,0);
        guides.matrix("inverseCurrent",camera.inverseJitteredViewProjection);guides.matrix("previousViewProjection",camera.previousViewProjection);
        guides.vector3("cameraDelta",camera.cameraDelta.x,camera.cameraDelta.y,camera.cameraDelta.z);
        guides.vector2("renderSize",surfaces[0],surfaces[1]);guides.vector2("jitterPx",camera.jitter.x,camera.jitter.y);
        guides.integer("resetHistory",camera.reset?1:0);GL30.glBindVertexArray(vao);GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
        cameraMetadata(camera,frameTime);
        var game=Minecraft.getInstance();
        int screen=game.screen!=null||game.getOverlay()!=null||game.isPaused()?1:4;
        int result=NativeBridge.evaluate(handle,frame,cameraData,camera.reset,screen);
        worldRendering=false;
        if(result==0&&nativeStatusAvailable&&NativeStatus.effectiveSr(nativeStatus)!=0)copyTexture(surfaces[7],display,true);
        else blit(source,display,GL11.GL_COLOR_BUFFER_BIT,GL11.GL_LINEAR);
        copyDepth(source,display,copyDepth,vao);
        if(result!=0) {
            status="Native evaluate failed: "+result;
            if(result!=lastResult||refusedFrames++<4||refusedFrames%600==0)ReScaleFrameCraft.LOGGER.error("Native evaluate refused frame {}: {}, near={}, far={}, fov={}, dt={}",frame,result,cameraData.get(84),cameraData.get(85),cameraData.get(86),cameraData.get(87));
        } else if(nativeStatusAvailable&&acceptedWidth!=source.viewWidth) {
            acceptedWidth=source.viewWidth;ReScaleFrameCraft.LOGGER.info("Native scene processed frame {}: {}x{} -> {}x{}, SR backend={}",frame,source.viewWidth,source.viewHeight,surfaces[2],surfaces[3],NativeStatus.effectiveSr(nativeStatus));
        }
        lastResult=result;
        // Preserve a display-sized completed scene before hand/HUD writes. Native owns its copy.
        copyToShared(display,surfaces[8]);NativeBridge.hudless(handle,frame,cameraData);
    }
    static FloatBuffer cameraMetadata(TemporalCamera c,float seconds) {
        // JOML column-major arrays are the row-major row-vector matrices used by the C ABI.
        projection.set(remap).mul(TemporalCamera.canonicalDepth(c.reversedDepth,c.zeroToOne,depthRemap)).mul(c.projection);
        inverseProjection.set(projection).invert();
        viewToWorld.set(c.view).invert().setTranslation((float)c.position.x,(float)c.position.y,(float)c.position.z);
        worldToView.set(viewToWorld).invert();
        currentInverse.set(c.viewProjection).invert();
        previousClip.set(c.previousViewProjection).translate(c.cameraDelta);
        previousClip.mulLocal(remap).mul(currentInverse).mul(inverseRemap);
        cameraData.clear();put(projection);put(inverseProjection);put(viewToWorld);put(worldToView);put(previousClip);
        cameraData.put(c.jitter.x).put(-c.jitter.y).put(c.previousJitter.x).put(-c.previousJitter.y);
        float a=projection.m22(),b=projection.m32();
        float far=Math.abs(b/(a+1));
        cameraData.put(Math.abs(b/a)).put(Float.isFinite(far)?far:0).put(2*(float)Math.atan(1/Math.abs(projection.m11()))).put(seconds);cameraData.flip();
        return cameraData;
    }
    private static void put(Matrix4f matrix) {
        int offset=cameraData.position();matrix.get(offset,cameraData);cameraData.position(offset+16);
    }
    private static void attach(int texture) {
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,readFramebuffer);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,texture,0);
        GL11.glDrawBuffer(GL30.GL_COLOR_ATTACHMENT0);GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);
    }
    private static void copyTexture(int texture,RenderTarget destination,boolean flip) {
        attach(texture);GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,destination.frameBufferId);
        GL30.glBlitFramebuffer(0,flip?surfaces[3]:0,surfaces[2],flip?0:surfaces[3],0,0,destination.viewWidth,destination.viewHeight,GL11.GL_COLOR_BUFFER_BIT,GL11.GL_NEAREST);
    }
    private static void copyToShared(RenderTarget source,int texture) {
        attach(texture);GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,source.frameBufferId);
        GL30.glBlitFramebuffer(0,0,source.viewWidth,source.viewHeight,0,surfaces[3],surfaces[2],0,GL11.GL_COLOR_BUFFER_BIT,GL11.GL_NEAREST);
    }
    private static void blit(RenderTarget source,RenderTarget target,int mask,int filter) {
        GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,source.frameBufferId);GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,target.frameBufferId);
        GL30.glBlitFramebuffer(0,0,source.viewWidth,source.viewHeight,0,0,target.viewWidth,target.viewHeight,mask,filter);
    }
    static void copyDepth(RenderTarget source,RenderTarget destination,GlProgram program,int vertexArray) {
        // Iris/other mods can enable packed stencil on the display target while
        // the scaled world target has depth only. GL depth blits require identical
        // formats; a depth draw converts through the destination's actual format.
        GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,destination.frameBufferId);
        RenderSystem.viewport(0,0,destination.viewWidth,destination.viewHeight);
        RenderSystem.enableDepthTest();RenderSystem.depthFunc(GL11.GL_ALWAYS);RenderSystem.depthMask(true);
        RenderSystem.colorMask(false,false,false,false);
        program.use();program.integer("sourceDepth",0);program.vector2("destinationSize",destination.viewWidth,destination.viewHeight);
        TextureUnits.bind(0,source.getDepthTextureId());GL33.glBindSampler(0,0);GL30.glBindVertexArray(vertexArray);
        GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
        RenderSystem.colorMask(true,true,true,true);RenderSystem.disableDepthTest();RenderSystem.depthMask(false);
    }
    public static void restoreDisplay() {if(display!=null&&handle!=0&&surfaces!=null) {worldRendering=false;display.bindWrite(true);}}
    public static boolean present() {
        if(handle==0||surfaces==null)return false;
        // GLFW can recursively updateDisplay while dispatching a resize. Neither
        // that nested call nor a reallocated, unfinished main target is a frame.
        if(displayDepth>1||closedFrame==frame)return true;
        if(display.viewWidth!=frameDisplayWidth||display.viewHeight!=frameDisplayHeight) {abortFrame();return true;}
        try(var state=new GlState()) {
            state.fullscreen();copyToShared(display,surfaces[9]);
            // A menu still closes its real CPU frame. Absence of matching world
            // capture disables interpolation without leaking Streamline tokens.
            int result=NativeBridge.present(handle,frame,Minecraft.getInstance().options.enableVsync().get());
            if(result!=0)throw new IllegalStateException("Native presentation failed: "+result);
            closedFrame=frame;cpuFrames.ended(frame);return true;
        }
    }
    public static void beforePostPresentPoll() {
        if(handle!=0&&surfaces!=null&&displayDepth==1&&closedFrame==frame)
            inputs.begin(cpuFrames.beforeNextPoll(frame));
    }
    public static void beginDisplay() {++displayDepth;}
    public static void endDisplay() {--displayDepth;}
    public static String status() {
        if(status!=null)return status;
        return "SR "+name(NativeStatus.requestedSr(nativeStatus))+" -> "+name(NativeStatus.effectiveSr(nativeStatus))+" ("+NativeStatus.srResult(nativeStatus)+", frames="+NativeStatus.evaluated(nativeStatus)+"), FG "+name(NativeStatus.requestedFg(nativeStatus))+" -> "+name(NativeStatus.effectiveFg(nativeStatus))+
            ", active="+(NativeStatus.fgActive(nativeStatus)!=0)+", generated/real="+NativeStatus.effectiveGeneratedFrames(nativeStatus)+", tags="+NativeStatus.taggedFrames(nativeStatus)+", generated presented="+
            ((NativeStatus.fgValidStatistics(nativeStatus)&2)!=0?Long.toString(NativeStatus.generatedPresented(nativeStatus)):"n/a")+", reason="+NativeStatus.fgReason(nativeStatus)+", result="+NativeStatus.fgResult(nativeStatus)+", latency="+latencyStatus();
    }
    private static String latencyStatus() {
        if(NativeStatus.latencyAvailable(nativeStatus)==0)return "unavailable";
        return switch((int)NativeStatus.latencyOwner(nativeStatus)) {case 1->"Reflex";case 2->"XeLL";case 3->"AMD Anti-Lag 2";default->"none";}+
            ", paced="+NativeStatus.pacedFrames(nativeStatus)+", input="+NativeStatus.keyboardEvents(nativeStatus)+"/"+NativeStatus.mouseEvents(nativeStatus)+"/"+NativeStatus.controllerEvents(nativeStatus)+
            ", ping="+NativeStatus.pings(nativeStatus)+", substitutes="+NativeStatus.substitutes(nativeStatus)+", order violations="+NativeStatus.orderViolations(nativeStatus)+", derived="+NativeStatus.derivedBoundaries(nativeStatus);
    }
    private static void abortFrame() {NativeBridge.abort(handle,frame);closedFrame=frame;cpuFrames.ended(frame);}
    static TemporalRenderer.SceneFrame sceneFrame(long id) {return new TemporalRenderer.SceneFrame(id,surfaces[2],surfaces[3],surfaces[8],true);}
    public static void shutdown() {
        inputs.end();
        if(handle!=0) {
            int result=NativeBridge.close(handle);
            if(result==0)handle=0;
            else ReScaleFrameCraft.LOGGER.warn("Shared native teardown will retain its owner after a refusal: {}",result);
            surfaces=null;worldRendering=false;
        }
        if(handle==0&&surfaceWindow!=0) {NativeBridge.destroySurface(surfaceWindow);surfaceWindow=0;}
        if(world!=null) {world.destroyBuffers();world=null;}
        if(guides!=null) {guides.close();guides=null;}
        if(copyDepth!=null) {copyDepth.close();copyDepth=null;}
        if(framebuffer!=0)GL30.glDeleteFramebuffers(framebuffer);
        if(readFramebuffer!=0)GL30.glDeleteFramebuffers(readFramebuffer);
        if(vao!=0)GL30.glDeleteVertexArrays(vao);
        framebuffer=readFramebuffer=vao=0;
        cpuFrames.reset();
        applied=failed=null;display=null;status="native processing off";
        nativeStatusAvailable=false;
    }
}
