package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.vertex.VertexSorting;
import net.minecraft.client.Camera;
import net.minecraft.client.Minecraft;
import org.joml.Matrix4f;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.client.TaaSettings;
import org.kpc.rescaleframecraft.temporal.GeometryHistory;
import org.kpc.rescaleframecraft.temporal.CloudHistory;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.opengl.*;

public final class TemporalRenderer {
    public static final TemporalCamera CAMERA=new TemporalCamera();
    public static final GeometryHistory GEOMETRY=new GeometryHistory();
    public static final CloudHistory CLOUDS=new CloudHistory();
    private static TaaHistory history;
    private static MotionGuides motion;
    private static Matrix4f sceneProjection;
    private static boolean active,failed;
    private static String status="waiting for world";
    private static int loggedWidth,loggedHeight;
    /** Valid until the next begin/release; no hand or HUD is ever drawn into this texture. */
    public record SceneFrame(long frameId,int width,int height,int colorTexture,boolean topLeftOrigin) {}
    private static SceneFrame sceneFrame;
    private static long completedFrames;
    private TemporalRenderer() {}

    public static void begin(Camera camera,Matrix4f view,Matrix4f projection) {
        sceneFrame=null;
        if(active)reset();
        var minecraft=Minecraft.getInstance();
        if((!TaaSettings.current.enabled&&!NativeRenderer.processing())||failed||minecraft.level==null) { CAMERA.invalidate();return; }
        var target=minecraft.getMainRenderTarget();
        boolean clip=GL.getCapabilities().OpenGL45||GL.getCapabilities().GL_ARB_clip_control;
        boolean zeroToOne=clip&&GL11.glGetInteger(GL45.GL_CLIP_DEPTH_MODE)==GL45.GL_ZERO_TO_ONE;
        boolean reversed=GL11.glGetDouble(GL11.GL_DEPTH_CLEAR_VALUE)<0.5;
        var pos=camera.getPosition();
        CAMERA.phaseCount=NativeRenderer.phaseCount();
        CAMERA.jitterEnabled=TaaSettings.current.enabled||NativeRenderer.upscaling();
        CAMERA.begin(view,projection,pos.x,pos.y,pos.z,target.viewWidth,target.viewHeight,minecraft.level,reversed,zeroToOne);
        if(CAMERA.reset) { GEOMETRY.clear();CLOUDS.clear(); }
        sceneProjection=projection;
        projection.set(CAMERA.jitteredProjection);
        RenderSystem.setProjectionMatrix(projection,VertexSorting.DISTANCE_TO_ORIGIN);
        GEOMETRY.begin(CAMERA.frame);active=true;
    }
    public static void finish() {
        if(!active)return;
        var target=Minecraft.getInstance().getMainRenderTarget();
        boolean nativeScene=NativeRenderer.processing(),nativeSr=NativeRenderer.upscaling();
        try(var state=new GlState()) {
            state.fullscreen();
            initialize();
            if(!nativeSr&&TaaSettings.current.enabled)history.resize(target.viewWidth,target.viewHeight);
            motion.resize(target.viewWidth,target.viewHeight);
            motion.produce(target,CAMERA,GEOMETRY.finish(),CLOUDS);
            state.fullscreen();
            if(!nativeSr&&TaaSettings.current.enabled)history.resolve(target,CAMERA,motion.depth(),motion.motion());
            if(NativeRenderer.processing()) {
                state.fullscreen();NativeRenderer.finishWorld(target,CAMERA,motion.depth(),motion.motion());
            }
            // Publish the owned resolve output, never the main target which is
            // about to receive first-person hand, HUD, menus and overlays.
            sceneFrame=nativeScene?NativeRenderer.sceneFrame(++completedFrames):new SceneFrame(++completedFrames,target.viewWidth,target.viewHeight,history.sceneColor(),false);
            if(loggedWidth!=target.viewWidth||loggedHeight!=target.viewHeight) {
                status=(nativeSr?"Native SR":"TAA raw ring, 8 samples")+" "+target.viewWidth+"x"+target.viewHeight;
                ReScaleFrameCraft.LOGGER.info("Temporal world prepared: mode={}, {}x{}, jitter={}px, reverseDepth={}, zeroToOne={}, HUD/hand after resolve",
                    nativeSr?"native SDK":"raw-ring TAA",target.viewWidth,target.viewHeight,CAMERA.jitter,CAMERA.reversedDepth,CAMERA.zeroToOne);
                loggedWidth=target.viewWidth;loggedHeight=target.viewHeight;
            }
        } catch(Exception e) {
            sceneFrame=null;
            failed=true;status="TAA unavailable: "+e.getMessage();
            ReScaleFrameCraft.LOGGER.error("Temporal resolve failed; native rendering restored",e);
            CAMERA.invalidate();GEOMETRY.clear();
        } finally {
            active=false;
            if(sceneProjection!=null)sceneProjection.set(CAMERA.projection);
            RenderSystem.setProjectionMatrix(CAMERA.projection,VertexSorting.DISTANCE_TO_ORIGIN);
            NativeRenderer.restoreDisplay();
        }
    }
    public static boolean capturing() { return active; }
    public static SceneFrame sceneFrame() { return sceneFrame; }
    private static void initialize() {
        if(history==null)history=new TaaHistory();
        if(motion==null)motion=new MotionGuides();
    }
    public static void preflight() {
        if(history!=null&&motion!=null||failed||!TaaSettings.current.enabled)return;
        try(var state=new GlState()) {
            initialize();
            ReScaleFrameCraft.LOGGER.info("ReScaleFrameCraft GPU programs linked in modpack: {} / {}",GL11.glGetString(GL11.GL_RENDERER),GL11.glGetString(GL11.GL_VERSION));
        } catch(Exception e) {
            failed=true;status="TAA unavailable: "+e.getMessage();
            ReScaleFrameCraft.LOGGER.error("ReScaleFrameCraft GPU program initialization failed",e);
        }
    }
    public static String status() { return (!TaaSettings.current.enabled?"TAA off":status)+" | "+NativeRenderer.status(); }
    public static void reset() { CAMERA.invalidate();GEOMETRY.clear();CLOUDS.clear();failed=false;loggedWidth=loggedHeight=0; }
    public static void release() {
        sceneFrame=null;
        active=false;reset();
        if(history!=null) { history.close();history=null; }
        if(motion!=null) { motion.close();motion=null; }
        status="waiting for world";
    }
}
