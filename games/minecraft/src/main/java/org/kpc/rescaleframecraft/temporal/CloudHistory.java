package org.kpc.rescaleframecraft.temporal;

import com.mojang.blaze3d.vertex.VertexBuffer;
import org.joml.Matrix4f;

/** Retains the transforms of the actual cached cloud mesh, including cloud drift. */
public final class CloudHistory {
    public VertexBuffer buffer;
    public final Matrix4f current=new Matrix4f(),previous=new Matrix4f();
    public final Matrix4f projection=new Matrix4f(),modelView=new Matrix4f();
    public int texture;
    public float alpha=1;
    public boolean valid;
    private long revision,lastRevision;
    private long lastFrame=-1;
    public void capture(VertexBuffer mesh,Matrix4f modelView,Matrix4f projection,int texture,TemporalCamera camera) {
        capture(mesh,modelView,projection,texture,1,camera);
    }
    public void meshChanged(VertexBuffer mesh) { if(mesh==buffer)revision++; }
    public void capture(VertexBuffer mesh,Matrix4f modelView,Matrix4f projection,int texture,float alpha,TemporalCamera camera) {
        if(lastFrame==camera.frame)return; // Fancy clouds draw the same mesh twice.
        valid=buffer==mesh&&lastFrame==camera.frame-1&&revision==lastRevision&&!camera.reset;
        previous.set(current);
        this.projection.set(projection);this.modelView.set(modelView);
        // Previous transforms must exclude the previous frame's raster jitter.
        current.set(TemporalCamera.canonicalDepth(camera.reversedDepth,camera.zeroToOne)).mul(projection).mul(modelView);
        if(valid) {
            new Matrix4f().translate(-2*camera.previousJitter.x/camera.width(),-2*camera.previousJitter.y/camera.height(),0).mul(previous,previous);
        } else {
            new Matrix4f().translate(-2*camera.jitter.x/camera.width(),-2*camera.jitter.y/camera.height(),0).mul(current,previous);
        }
        buffer=mesh;this.texture=texture;this.alpha=alpha;lastFrame=camera.frame;lastRevision=revision;
    }
    public boolean present(long frame) { return lastFrame==frame&&buffer!=null&&!buffer.isInvalid(); }
    public void clear() { buffer=null;valid=false;lastFrame=-1; }
}
