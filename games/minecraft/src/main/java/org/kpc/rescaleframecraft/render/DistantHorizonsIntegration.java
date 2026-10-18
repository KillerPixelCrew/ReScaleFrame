package org.kpc.rescaleframecraft.render;

import java.io.IOException;
import com.seibel.distanthorizons.api.DhApi;
import com.seibel.distanthorizons.api.objects.math.DhApiMat4f;
import net.irisshaders.iris.Iris;
import net.irisshaders.iris.compat.dh.DHCompatInternal;
import net.irisshaders.iris.pipeline.IrisRenderingPipeline;
import org.joml.Matrix4f;
import org.joml.Matrix4fc;
import org.kpc.rescaleframecraft.ReScaleFrameCraft;
import org.kpc.rescaleframecraft.render.flywheel.IrisState;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;

/** Same-frame DH scene inputs. Referenced only when Distant Horizons and Iris are installed. */
public final class DistantHorizonsIntegration {
    private static final Matrix4f lodProjection=new Matrix4f(),canonical=new Matrix4f(),remap=new Matrix4f();
    private static long projectionFrame=-1;
    private static boolean deferred;
    private DistantHorizonsIntegration() {}
    public static boolean ready() {return DhApi.Delayed.configs!=null&&DhApi.Delayed.renderProxy!=null;}
    public static boolean enabled() {return ready()&&DhApi.Delayed.configs.graphics().renderingEnabled().getValue();}
    public static void defer() {deferred=true;}
    public static void beginFrame() {projectionFrame=-1;}
    public static void frame() {
        if(!deferred||!ready())return;
        deferred=false;
        var pipeline=Iris.getPipelineManager().getPipeline().orElse(null);
        if(!(pipeline instanceof IrisRenderingPipeline iris)||!IrisState.packActive()||iris.getDHCompat().getInstance()!=DHCompatInternal.SHADERLESS)return;
        try {Iris.reload();TemporalRenderer.reset();}
        catch(IOException failure) {ReScaleFrameCraft.LOGGER.error("Iris reload after Distant Horizons initialization failed",failure);}
    }
    public static void extendProjection(Matrix4f projection,boolean reversed,boolean zeroToOne) {
        if(!enabled())return;
        float far=(DhApi.Delayed.configs.graphics().chunkRenderDistance().getValue()*16f+512f)*(float)Math.sqrt(2);
        extendProjection(projection,far,reversed,zeroToOne,canonical,remap);
    }
    static void extendProjection(Matrix4f projection,float far,boolean reversed,boolean zeroToOne,Matrix4f canonical,Matrix4f remap) {
        TemporalCamera.canonicalDepth(reversed,zeroToOne,remap);
        canonical.set(remap).mul(projection);
        if(Math.abs(canonical.m23()+1)>0.0001f||Math.abs(canonical.m33())>0.0001f)return;
        float near=canonical.perspectiveNear();
        if(!Float.isFinite(near)||near<=0||far<=canonical.perspectiveFar())return;
        canonical.m22(-(far+near)/(far-near)).m32(-2*far*near/(far-near));
        projection.set(remap.invert()).mul(canonical);
    }
    public static Matrix4fc lodProjection(Matrix4fc original) {
        if(!TemporalRenderer.capturing())return original;
        var camera=TemporalRenderer.CAMERA;
        jitterProjection(original,lodProjection);
        projectionFrame=camera.frame;
        return lodProjection;
    }
    public static Matrix4f jitterProjection(Matrix4fc original,Matrix4f destination) {
        var camera=TemporalRenderer.CAMERA;
        return destination.identity().translate(2*camera.jitter.x/camera.width(),2*camera.jitter.y/camera.height(),0).mul(original);
    }
    public static void captureProjection(DhApiMat4f projection) {
        if(!TemporalRenderer.capturing()||IrisState.packActive())return;
        lodProjection.set(projection.m00,projection.m10,projection.m20,projection.m30,
            projection.m01,projection.m11,projection.m21,projection.m31,
            projection.m02,projection.m12,projection.m22,projection.m32,
            projection.m03,projection.m13,projection.m23,projection.m33);
        projectionFrame=TemporalRenderer.CAMERA.frame;
    }
    public static int depth() {
        if(!enabled()||projectionFrame!=TemporalRenderer.CAMERA.frame)return 0;
        if(!IrisState.packActive()) {
            var result=DhApi.Delayed.renderProxy.getDhDepthTextureId();
            return result.success&&result.payload!=null?result.payload:0;
        }
        var pipeline=Iris.getPipelineManager().getPipeline().orElse(null);
        if(!(pipeline instanceof IrisRenderingPipeline iris)||!(iris.getDHCompat().getInstance() instanceof DHCompatInternal compat)||!compat.shouldOverride)return 0;
        return iris.getDHCompat().getDepthTex();
    }
    public static void inverseProjection(Matrix4f result) {result.set(lodProjection).invert();}
}
