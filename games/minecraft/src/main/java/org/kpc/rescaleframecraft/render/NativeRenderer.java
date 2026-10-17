package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.pipeline.TextureTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import java.nio.FloatBuffer;
import java.util.concurrent.locks.LockSupport;
import java.util.function.Supplier;
import net.minecraft.client.Minecraft;
import org.joml.Matrix4f;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.client.TaaSettings;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.glfw.GLFWNativeWin32;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryUtil;

/** Game ownership, render resolution, input conventions and HUD boundaries live here. */
public final class NativeRenderer {
    private static long handle,surfaceWindow,frame,lastFrameTime,lastPacing;
    private static String settingsKey="",failedKey="",status="native processing off";
    private static int[] surfaces;
    private static long[] nativeStatus;
    private static RenderTarget display;
    private static TextureTarget world;
    private static boolean worldRendering,worldFinished;
    private static GlProgram guides;
    private static int framebuffer,readFramebuffer,vao;
    private static int displayDepth,frameDisplayWidth,frameDisplayHeight;
    private static final FloatBuffer cameraData=MemoryUtil.memAllocFloat(88);
    private static float frameTime=.016f;
    private NativeRenderer() {}
    public static void beginFrame() {
        var game=Minecraft.getInstance();var s=TaaSettings.current;
        long now=System.nanoTime();
        if(handle!=0&&s.frameLimit>0&&lastPacing!=0) {
            long remaining=lastPacing+1_000_000_000L/s.frameLimit-now;
            if(remaining>0)LockSupport.parkNanos(remaining);
            now=System.nanoTime();
        }
        lastPacing=now;frameTime=lastFrameTime==0?.016f:Math.clamp((now-lastFrameTime)*1e-9f,.001f,1f);lastFrameTime=now;
        ++frame;worldFinished=false;worldRendering=false;
        if(!GL.getCapabilities().OpenGL33)return;
        display=game.getMainRenderTarget();
        frameDisplayWidth=display.viewWidth;frameDisplayHeight=display.viewHeight;
        String key=display.viewWidth+"x"+display.viewHeight+":"+s.upscaler+":"+s.quality+":"+s.frameGeneration+":"+s.generatedFrames+":"+s.reflex+":"+s.frameLimit+":"+s.nativeDebug;
        if(handle==0&&s.upscaler==TaaSettings.Upscaler.OFF&&s.frameGeneration==TaaSettings.FrameGeneration.OFF)return;
        if(!key.equals(settingsKey)&&!key.equals(failedKey)) {
            try(var saved=new GlState(true)) {
                if(handle==0) {
                    var root=NativeBridge.load();
                    long parent=GLFWNativeWin32.glfwGetWin32Window(game.getWindow().getWindow());
                    surfaceWindow=windowThread(()->NativeBridge.createSurface(parent));
                    if(surfaceWindow==0)throw new IllegalStateException("Presentation surface must be created on the GLFW event thread");
                    handle=NativeBridge.open(parent,surfaceWindow,root.toString());
                    if(handle==0)throw new IllegalStateException("Native device/presentation initialization refused; see minecraft-native.log");
                    guides=new GlProgram("fullscreen.vert","native-guides.frag");
                    framebuffer=GL30.glGenFramebuffers();readFramebuffer=GL30.glGenFramebuffers();vao=GL30.glGenVertexArrays();
                }
                var replacement=NativeBridge.configure(handle,display.viewWidth,display.viewHeight,s.upscaler.id,s.quality.ordinal(),s.frameGeneration.id,
                    s.generatedFrames,s.reflex.ordinal(),s.frameLimit,s.nativeDebug);
                if(replacement==null)throw new IllegalStateException("Native reconfiguration failed; see minecraft-native.log");
                boolean resize=surfaces==null||surfaces[0]!=replacement[0]||surfaces[1]!=replacement[1];surfaces=replacement;
                if(resize) {
                    if(world!=null)world.destroyBuffers();
                    world=new TextureTarget(surfaces[0],surfaces[1],true,Minecraft.ON_OSX);
                }
                settingsKey=key;failedKey="";TemporalRenderer.reset();
                ReScaleFrameCraft.LOGGER.info("ReScaleFrame native settings applied: {}, world {}x{}, display {}x{}",key,surfaces[0],surfaces[1],surfaces[2],surfaces[3]);
            } catch(Exception|LinkageError error) {
                failedKey=key;status=error.getMessage();ReScaleFrameCraft.LOGGER.error("ReScaleFrame native configuration failed",error);
            }
        }
        if(handle!=0&&surfaces!=null) {
            NativeBridge.cpu(handle,frame,0);NativeBridge.cpu(handle,frame,6);NativeBridge.cpu(handle,frame,1);NativeBridge.cpu(handle,frame,2);
            nativeStatus=NativeBridge.status(handle);
            status="SR "+name(nativeStatus[0])+" -> "+name(nativeStatus[1])+" ("+nativeStatus[4]+"), FG "+name(nativeStatus[2])+" -> "+name(nativeStatus[3])+
                ", generated/real="+nativeStatus[9]+", tags="+nativeStatus[10]+", presented="+nativeStatus[11]+", reason="+nativeStatus[7]+", result="+nativeStatus[8];
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
    public static void simulationEnd() {if(handle!=0)NativeBridge.cpu(handle,frame,3);}
    public static void beginWorld() {
        if(handle==0||surfaces==null||world==null)return;
        worldRendering=true;world.setClearColor(0,0,0,0);world.clear(Minecraft.ON_OSX);world.bindWrite(true);
    }
    public static RenderTarget worldTarget() {return worldRendering?world:null;}
    public static boolean processing() {return worldRendering&&surfaces!=null;}
    public static boolean upscaling() {return processing()&&nativeStatus!=null&&nativeStatus[1]!=0;}
    public static int phaseCount() {return upscaling()?Math.max(8,Math.round(8f*surfaces[2]*surfaces[3]/surfaces[0]/surfaces[1])):8;}
    static void finishWorld(RenderTarget source,TemporalCamera camera,int depth,int motion) {
        if(!processing())return;
        // Three shared MRTs carry scene colour, canonical depth and dense unjittered motion.
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,framebuffer);
        for(int i=0;i<3;i++)GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0+i,GL11.GL_TEXTURE_2D,surfaces[4+i],0);
        GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1,GL30.GL_COLOR_ATTACHMENT2});
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
        packCamera(camera);
        int screen=Minecraft.getInstance().screen!=null||Minecraft.getInstance().isPaused()?1:4;
        int result=NativeBridge.evaluate(handle,frame,cameraData,camera.reset,screen);
        worldRendering=false;
        if(result==0&&nativeStatus!=null&&nativeStatus[1]!=0)copyTexture(surfaces[7],display,true);
        else blit(source,display,GL11.GL_COLOR_BUFFER_BIT,GL11.GL_LINEAR);
        blit(source,display,GL11.GL_DEPTH_BUFFER_BIT,GL11.GL_NEAREST);
        if(result!=0) {status="Native evaluate failed: "+result;ReScaleFrameCraft.LOGGER.error("Native evaluate refused frame {}: {}",frame,result);}
        // Preserve a display-sized completed scene before hand/HUD writes. Native owns its copy.
        copyToShared(display,surfaces[8]);NativeBridge.hudless(handle,frame,cameraData);worldFinished=true;
    }
    private static void packCamera(TemporalCamera c) {
        // JOML column-major arrays are the row-major row-vector matrices used by the C ABI.
        Matrix4f remap=new Matrix4f().m11(-1).m22(.5f).m32(.5f);
        Matrix4f projection=new Matrix4f(remap).mul(TemporalCamera.canonicalDepth(c.reversedDepth,c.zeroToOne)).mul(c.projection);
        Matrix4f inverse=new Matrix4f(projection).invert();
        Matrix4f viewToWorld=new Matrix4f(c.view).invert().setTranslation((float)c.position.x,(float)c.position.y,(float)c.position.z);
        Matrix4f currentInverse=new Matrix4f(c.viewProjection).invert();
        Matrix4f previous=new Matrix4f(c.previousViewProjection).translate(c.cameraDelta);
        Matrix4f previousClip=new Matrix4f(remap).mul(previous).mul(currentInverse).mul(new Matrix4f(remap).invert());
        cameraData.clear();put(projection);put(inverse);put(viewToWorld);put(new Matrix4f(viewToWorld).invert());put(previousClip);
        cameraData.put(c.jitter.x).put(-c.jitter.y).put(c.previousJitter.x).put(-c.previousJitter.y);
        float a=projection.m22(),b=projection.m32();
        cameraData.put(Math.abs(b/a)).put(Math.abs(b/(a+1))).put(2*(float)Math.atan(1/Math.abs(projection.m11()))).put(frameTime);cameraData.flip();
    }
    private static void put(Matrix4f matrix) {float[] values=new float[16];matrix.get(values);cameraData.put(values);}
    private static void attach(int texture) {
        GL30.glBindFramebuffer(GL30.GL_FRAMEBUFFER,readFramebuffer);
        GL30.glFramebufferTexture2D(GL30.GL_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,texture,0);
        GL20.glDrawBuffers(new int[]{GL30.GL_COLOR_ATTACHMENT0});GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);
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
    public static void restoreDisplay() {if(display!=null&&handle!=0&&surfaces!=null) {worldRendering=false;display.bindWrite(true);}}
    public static boolean present() {
        if(handle==0||surfaces==null)return false;
        // GLFW can recursively updateDisplay while dispatching a resize. Neither
        // that nested call nor a reallocated, unfinished main target is a frame.
        if(displayDepth>1||display.viewWidth!=frameDisplayWidth||display.viewHeight!=frameDisplayHeight)return true;
        try(var state=new GlState()) {
            state.fullscreen();copyToShared(display,surfaces[9]);
            // No matching world capture means menus/loading; frame 0 forbids interpolation.
            int result=NativeBridge.present(handle,worldFinished?frame:0,Minecraft.getInstance().options.enableVsync().get());
            if(result!=0)throw new IllegalStateException("Native presentation failed: "+result);
            NativeBridge.cpu(handle,frame,4);return true;
        }
    }
    public static void beginDisplay() {++displayDepth;}
    public static void endDisplay() {--displayDepth;}
    public static String status() {return status;}
    static TemporalRenderer.SceneFrame sceneFrame(long id) {return new TemporalRenderer.SceneFrame(id,surfaces[2],surfaces[3],surfaces[8],true);}
    public static void shutdown() {
        if(handle!=0) {NativeBridge.close(handle);handle=0;surfaces=null;worldRendering=false;}
        if(surfaceWindow!=0) {NativeBridge.destroySurface(surfaceWindow);surfaceWindow=0;}
    }
}
