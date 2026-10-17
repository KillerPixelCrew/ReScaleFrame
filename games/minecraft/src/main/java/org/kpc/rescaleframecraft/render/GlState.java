package org.kpc.rescaleframecraft.render;

import com.mojang.blaze3d.systems.RenderSystem;
import net.minecraft.client.renderer.ShaderInstance;
import org.joml.Matrix4f;
import org.lwjgl.opengl.*;
import org.lwjgl.system.MemoryStack;

/** Restores the Minecraft state disturbed by the complete off-screen temporal pass. */
final class GlState implements AutoCloseable {
    private final boolean retiringResources;
    private final int program=GL11.glGetInteger(GL20.GL_CURRENT_PROGRAM);
    private final int vao=GL11.glGetInteger(GL30.GL_VERTEX_ARRAY_BINDING);
    private final int array=GL11.glGetInteger(GL15.GL_ARRAY_BUFFER_BINDING);
    private final int read=GL11.glGetInteger(GL30.GL_READ_FRAMEBUFFER_BINDING);
    private final int draw=GL11.glGetInteger(GL30.GL_DRAW_FRAMEBUFFER_BINDING);
    private final int active=GL11.glGetInteger(GL13.GL_ACTIVE_TEXTURE);
    private final boolean depth=GL11.glIsEnabled(GL11.GL_DEPTH_TEST);
    private final boolean blend=GL11.glIsEnabled(GL11.GL_BLEND);
    private final boolean cull=GL11.glIsEnabled(GL11.GL_CULL_FACE);
    private final boolean scissor=GL11.glIsEnabled(GL11.GL_SCISSOR_TEST);
    private final boolean depthMask=GL11.glGetBoolean(GL11.GL_DEPTH_WRITEMASK);
    private final boolean[] colorMask=new boolean[4];
    private final int depthFunc=GL11.glGetInteger(GL11.GL_DEPTH_FUNC);
    private final int[] viewport=new int[4];
    private final int[] textures=new int[16],samplers=new int[16];
    private final int[] shaderTextures=new int[16];
    private final ShaderInstance shader=RenderSystem.getShader();
    private final Matrix4f textureMatrix=new Matrix4f(RenderSystem.getTextureMatrix());
    private final float[] shaderColor=RenderSystem.getShaderColor().clone();
    private final int srcRgb=GL11.glGetInteger(GL14.GL_BLEND_SRC_RGB),dstRgb=GL11.glGetInteger(GL14.GL_BLEND_DST_RGB);
    private final int srcAlpha=GL11.glGetInteger(GL14.GL_BLEND_SRC_ALPHA),dstAlpha=GL11.glGetInteger(GL14.GL_BLEND_DST_ALPHA);
    private final int origin,clipDepth;
    private final boolean clipControl=GL.getCapabilities().OpenGL45||GL.getCapabilities().GL_ARB_clip_control;
    GlState() {this(false);}
    GlState(boolean retiringResources) {
        this.retiringResources=retiringResources;
        try(var stack=MemoryStack.stackPush()) {
            var mask=stack.malloc(4);GL11.glGetBooleanv(GL11.GL_COLOR_WRITEMASK,mask);
            for(int i=0;i<4;i++)colorMask[i]=mask.get(i)!=0;
        }
        origin=clipControl?GL11.glGetInteger(GL45.GL_CLIP_ORIGIN):GL45.GL_LOWER_LEFT;
        clipDepth=clipControl?GL11.glGetInteger(GL45.GL_CLIP_DEPTH_MODE):GL45.GL_NEGATIVE_ONE_TO_ONE;
        GL11.glGetIntegerv(GL11.GL_VIEWPORT,viewport);
        for(int i=0;i<16;i++) {
            GL13.glActiveTexture(GL13.GL_TEXTURE0+i);
            textures[i]=GL11.glGetInteger(GL11.GL_TEXTURE_BINDING_2D);
            samplers[i]=GL30.glGetIntegeri(GL33.GL_SAMPLER_BINDING,i);
            shaderTextures[i]=RenderSystem.getShaderTexture(i);
        }
        GL13.glActiveTexture(active);
    }
    void fullscreen() {
        RenderSystem.disableDepthTest();RenderSystem.depthMask(false);
        RenderSystem.disableBlend();RenderSystem.disableCull();
        RenderSystem.colorMask(true,true,true,true);
        GL11.glDisable(GL11.GL_SCISSOR_TEST);
    }
    @Override public void close() {
        GL20.glUseProgram(program);GL30.glBindVertexArray(vao);
        RenderSystem.setShader(()->shader);
        RenderSystem.setTextureMatrix(textureMatrix);
        RenderSystem.setShaderColor(shaderColor[0],shaderColor[1],shaderColor[2],shaderColor[3]);
        RenderSystem.blendFuncSeparate(srcRgb,dstRgb,srcAlpha,dstAlpha);
        if(clipControl)GL45.glClipControl(origin,clipDepth);
        GL15.glBindBuffer(GL15.GL_ARRAY_BUFFER,array);
        GL30.glBindFramebuffer(GL30.GL_READ_FRAMEBUFFER,retiringResources&&read!=0&&!GL30.glIsFramebuffer(read)?0:read);
        GL30.glBindFramebuffer(GL30.GL_DRAW_FRAMEBUFFER,retiringResources&&draw!=0&&!GL30.glIsFramebuffer(draw)?0:draw);
        RenderSystem.viewport(viewport[0],viewport[1],viewport[2],viewport[3]);
        if(depth)RenderSystem.enableDepthTest();else RenderSystem.disableDepthTest();
        if(blend)RenderSystem.enableBlend();else RenderSystem.disableBlend();
        if(cull)RenderSystem.enableCull();else RenderSystem.disableCull();
        RenderSystem.depthMask(depthMask);RenderSystem.depthFunc(depthFunc);
        RenderSystem.colorMask(colorMask[0],colorMask[1],colorMask[2],colorMask[3]);
        if(scissor)GL11.glEnable(GL11.GL_SCISSOR_TEST);else GL11.glDisable(GL11.GL_SCISSOR_TEST);
        for(int i=0;i<16;i++) {
            // Reconfiguration may retire a texture captured on entry. New names
            // are allocated before old names are released, so a retired snapshot
            // cannot accidentally alias the replacement.
            int shaderTexture=shaderTextures[i],texture=textures[i];
            if(retiringResources) {
                if(shaderTexture!=0&&!GL11.glIsTexture(shaderTexture))shaderTexture=0;
                if(texture!=0&&!GL11.glIsTexture(texture))texture=0;
            }
            RenderSystem.setShaderTexture(i,shaderTexture);
            TextureUnits.bind(i,texture);
            GL33.glBindSampler(i,samplers[i]);
        }
        RenderSystem.activeTexture(active);
    }
}
