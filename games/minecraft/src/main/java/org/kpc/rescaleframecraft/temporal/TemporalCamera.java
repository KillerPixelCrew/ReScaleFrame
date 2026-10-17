package org.kpc.rescaleframecraft.temporal;

import org.joml.Matrix4f;
import org.joml.Vector2f;
import org.joml.Vector3d;
import org.joml.Vector3f;
import org.joml.Vector4f;

/** One world-view history. All matrices consumed by TAA use canonical OpenGL depth. */
public final class TemporalCamera {
    public final Matrix4f projection = new Matrix4f();
    public final Matrix4f jitteredProjection = new Matrix4f();
    public final Matrix4f view = new Matrix4f();
    public final Matrix4f inverseJitteredViewProjection = new Matrix4f();
    public final Matrix4f jitteredViewProjection = new Matrix4f();
    public final Matrix4f previousViewProjection = new Matrix4f();
    public final Matrix4f viewProjection = new Matrix4f();
    public final Vector3d position = new Vector3d();
    public final Vector3f cameraDelta = new Vector3f();
    public final Vector2f jitter = new Vector2f();
    public final Vector2f previousJitter = new Vector2f();
    public boolean reset = true;
    public boolean reversedDepth;
    public boolean zeroToOne;
    public long frame;
    private boolean valid;
    private int width, height;
    private Object world;
    public int phaseCount=8;
    public boolean jitterEnabled=true;
    private final Matrix4f previousProjection = new Matrix4f();

    public void begin(Matrix4f worldView, Matrix4f worldProjection, double x, double y, double z,
                      int newWidth, int newHeight, Object worldIdentity, boolean reversed, boolean z01) {
        reset = !valid || width != newWidth || height != newHeight || world != worldIdentity
            || position.distanceSquared(x, y, z) > 64.0
            || reversedDepth != reversed || zeroToOne != z01
            || Math.abs(previousProjection.m00() - worldProjection.m00()) > 0.01f
            || Math.abs(previousProjection.m11() - worldProjection.m11()) > 0.01f;
        previousViewProjection.set(viewProjection);
        previousJitter.set(jitter);
        cameraDelta.set((float)(x-position.x), (float)(y-position.y), (float)(z-position.z));
        position.set(x,y,z);
        projection.set(worldProjection);
        previousProjection.set(worldProjection);
        view.set(worldView);
        width=newWidth; height=newHeight; world=worldIdentity;
        reversedDepth=reversed; zeroToOne=z01;
        if (reset) frame=0;
        int phase=(int)(frame % Math.max(1,phaseCount))+1;
        jitter.set(halton(phase,2)-0.5f,halton(phase,3)-0.5f);
        if(!jitterEnabled)jitter.zero();
        jitteredProjection.identity().translate(2*jitter.x/width,2*jitter.y/height,0).mul(projection);
        canonicalDepth(reversed,z01,viewProjection).mul(projection).mul(view);
        canonicalDepth(reversed,z01,jitteredViewProjection).mul(jitteredProjection).mul(view);
        inverseJitteredViewProjection.set(jitteredViewProjection).invert();
        if(reset) {
            previousViewProjection.set(viewProjection);
            previousJitter.set(jitter);
            cameraDelta.zero();
        }
        valid=true;
        frame++;
    }

    public void invalidate() { valid=false; reset=true; }
    public int width() { return width; }
    public int height() { return height; }

    public static Matrix4f canonicalDepth(boolean reversed, boolean zeroToOne) {
        return canonicalDepth(reversed,zeroToOne,new Matrix4f());
    }

    public static Matrix4f canonicalDepth(boolean reversed, boolean zeroToOne, Matrix4f destination) {
        float scale=zeroToOne?2:1;
        float bias=zeroToOne?-1:0;
        if(reversed) { scale=-scale; bias=-bias; }
        return destination.identity().m22(scale).m32(bias);
    }

    public static float halton(int index,int radix) {
        float value=0, fraction=1;
        while(index>0) { fraction/=radix; value+=fraction*(index%radix); index/=radix; }
        return value;
    }

    /** Homogeneous far-minus-near cancels the eye offset, including infinite far planes. */
    public static Vector3f skyDirection(Matrix4f inverse, float ndcX, float ndcY) {
        Vector4f far=inverse.transform(new Vector4f(ndcX,ndcY,1,1));
        Vector4f near=inverse.transform(new Vector4f(ndcX,ndcY,-1,1));
        Vector3f direction=new Vector3f(far.x*near.w-near.x*far.w,
            far.y*near.w-near.y*far.w,far.z*near.w-near.z*far.w);
        if((far.w<0)!=(near.w<0))direction.negate();
        return direction;
    }
}
