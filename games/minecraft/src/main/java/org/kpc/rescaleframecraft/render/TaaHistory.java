package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.pipeline.RenderTarget;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.blaze3d.platform.GlStateManager;
import java.util.ArrayList;
import java.util.List;
import org.joml.Matrix4f;
import org.joml.Vector2f;
import org.joml.Vector3d;
import org.kpc.rescaleframecraft.temporal.TemporalCamera;
import org.lwjgl.opengl.*;

/** Eight unique raw frames. Only coordinates travel through history, never filtered colour. */
public final class TaaHistory implements AutoCloseable {
    private static final class Frame {
        int color,depth,motion;
        final Matrix4f vp=new Matrix4f(),inverse=new Matrix4f(),view=new Matrix4f();
        final Vector2f jitter=new Vector2f();
        final Vector3d position=new Vector3d();
        void capture(TemporalCamera camera) {
            vp.set(camera.viewProjection);inverse.set(camera.inverseJitteredViewProjection);
            view.set(camera.view);jitter.set(camera.jitter);position.set(camera.position);
        }
    }
    private final GlProgram accumulate=new GlProgram("fullscreen.vert","taa-accumulate.frag");
    private final GlProgram output=new GlProgram("fullscreen.vert","taa-output.frag");
    private final List<Integer> textures=new ArrayList<>();
    private final int vao=GL30.glGenVertexArrays(),fbo=GL30.glGenFramebuffers(),copyFbo=GL30.glGenFramebuffers();
    private final Frame[] frames=new Frame[8];
    private final int[] sums=new int[2],coordinates=new int[2];
    private static final int[] DRAW_PAIR={GL30.GL_COLOR_ATTACHMENT0,GL30.GL_COLOR_ATTACHMENT1};
    private int width,height,head,available,result;
    /** Hand and HUD are composed on the main target, never on this owned texture. */
    int sceneColor() { return result; }
    public void resize(int w,int h) {
        if(w==width&&h==height)return;
        releaseTextures();width=w;height=h;head=available=0;
        for(int i=0;i<8;i++) {
            Frame frame=frames[i]=new Frame();
            frame.color=texture(GL30.GL_RGBA16F,GL11.GL_RGBA);
            frame.depth=texture(GL30.GL_R32F,GL11.GL_RED);
            frame.motion=texture(GL30.GL_RGBA16F,GL11.GL_RGBA);
        }
        for(int i=0;i<2;i++) {
            sums[i]=texture(GL30.GL_RGBA32F,GL11.GL_RGBA);
            coordinates[i]=texture(GL30.GL_RGBA32F,GL11.GL_RGBA);
        }
        result=texture(GL30.GL_RGBA16F,GL11.GL_RGBA);
    }
    public void resolve(RenderTarget target,TemporalCamera camera,int sceneDepth,int motion) {
        if(camera.reset)available=0;
        Frame current=frames[head];current.capture(camera);
        GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,target.frameBufferId);
        GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);copy(current.color);
        copyTexture(sceneDepth,current.depth);copyTexture(motion,current.motion);
        available=Math.min(available+1,8);
        accumulate.use();accumulate.vector2("renderSize",width,height);
        bind(accumulate,"currentColor",2,current.color);
        bind(accumulate,"currentDepth",3,current.depth);bind(accumulate,"currentMotion",4,current.motion);
        accumulate.matrix("inverseCurrent",current.inverse);accumulate.matrix("currentView",current.view);
        accumulate.vector2("currentJitter",current.jitter.x,current.jitter.y);
        accumulate.integer("stage",0);attachments(sums[0],coordinates[0]);draw();
        int bank=0;
        for(int age=1;age<available;age++) {
            Frame from=frames[(head-age+1+8)%8],previous=frames[(head-age+8)%8];
            int next=1-bank;
            accumulate.integer("stage",1);
            bind(accumulate,"accumulation",0,sums[bank]);bind(accumulate,"coordinateMap",1,coordinates[bank]);
            bind(accumulate,"currentDepth",3,from.depth);bind(accumulate,"currentMotion",4,from.motion);
            bind(accumulate,"previousColor",5,previous.color);bind(accumulate,"previousDepth",6,previous.depth);
            accumulate.matrix("inverseCurrent",from.inverse);accumulate.matrix("previousViewProjection",previous.vp);
            accumulate.matrix("currentView",from.view);
            accumulate.matrix("inversePrevious",previous.inverse);accumulate.matrix("previousView",previous.view);
            accumulate.vector2("currentJitter",from.jitter.x,from.jitter.y);
            accumulate.vector2("previousJitter",previous.jitter.x,previous.jitter.y);
            accumulate.vector3("cameraDelta",(float)(from.position.x-previous.position.x),
                (float)(from.position.y-previous.position.y),(float)(from.position.z-previous.position.z));
            attachments(sums[next],coordinates[next]);draw();bank=next;
        }
        output.use();output.vector2("renderSize",width,height);
        output.vector2("jitterPx",current.jitter.x,current.jitter.y);
        bind(output,"accumulation",0,sums[bank]);bind(output,"currentColor",1,current.color);
        bind(output,"currentDepth",2,current.depth);bind(output,"currentMotion",3,current.motion);
        attachments(result,0);draw();
        GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,fbo);GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);
        GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,target.frameBufferId);
        GL30.glBlitFramebuffer(0,0,width,height,0,0,width,height,GL11.GL_COLOR_BUFFER_BIT,GL11.GL_NEAREST);
        head=(head+1)%8;
    }
    private void copyTexture(int source,int destination) {
        GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,copyFbo);
        GL30.glFramebufferTexture2D(GL30.GL_READ_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,source,0);
        GL11.glReadBuffer(GL30.GL_COLOR_ATTACHMENT0);copy(destination);
    }
    private void copy(int destination) {
        TextureUnits.bind(0,destination);
        GL11.glCopyTexSubImage2D(GL11.GL_TEXTURE_2D,0,0,0,0,0,width,height);
    }
    private static void bind(GlProgram program,String name,int unit,int texture) {
        TextureUnits.bind(unit,texture);GL33.glBindSampler(unit,0);program.integer(name,unit);
    }
    private void draw() {
        RenderSystem.viewport(0,0,width,height);GL30.glBindVertexArray(vao);
        GL11.glDrawArrays(GL11.GL_TRIANGLES,0,3);
    }
    private void attachments(int color,int coordinates) {
        GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,fbo);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT0,GL11.GL_TEXTURE_2D,color,0);
        GL30.glFramebufferTexture2D(GL30.GL_DRAW_FRAMEBUFFER,GL30.GL_COLOR_ATTACHMENT1,GL11.GL_TEXTURE_2D,coordinates,0);
        if(coordinates==0)GL11.glDrawBuffer(GL30.GL_COLOR_ATTACHMENT0);else GL20.glDrawBuffers(DRAW_PAIR);
        if(GL30.glCheckFramebufferStatus(GL30.GL_DRAW_FRAMEBUFFER)!=GL30.GL_FRAMEBUFFER_COMPLETE)
            throw new IllegalStateException("Incomplete raw TAA framebuffer");
    }
    private int texture(int format,int external) {
        int texture=GL11.glGenTextures();textures.add(texture);TextureUnits.bind(0,texture);
        GL11.glTexImage2D(GL11.GL_TEXTURE_2D,0,format,width,height,0,external,GL11.GL_FLOAT,0L);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MIN_FILTER,GL11.GL_NEAREST);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_MAG_FILTER,GL11.GL_NEAREST);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_WRAP_S,GL12.GL_CLAMP_TO_EDGE);
        GL11.glTexParameteri(GL11.GL_TEXTURE_2D,GL11.GL_TEXTURE_WRAP_T,GL12.GL_CLAMP_TO_EDGE);return texture;
    }
    private void releaseTextures() { for(int texture:textures)GlStateManager._deleteTexture(texture);textures.clear(); }
    @Override public void close() {
        releaseTextures();accumulate.close();output.close();GL30.glDeleteVertexArrays(vao);
        GL30.glDeleteFramebuffers(fbo);GL30.glDeleteFramebuffers(copyFbo);
    }
}
