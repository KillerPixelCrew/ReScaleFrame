package org.kpc.rescaleframecraft.render.flywheel;

import java.nio.IntBuffer;
import org.lwjgl.opengl.*;

/** GPU-resident temporal vertices or index tables. The GL stream orders writes and subsequent reads. */
final class GpuHistoryBuffer implements AutoCloseable {
    private final int buffer=GL15.glGenBuffers(),texture=GL11.glGenTextures();
    private final int format;
    private long capacity;
    GpuHistoryBuffer(int format) {this.format=format;}
    int buffer() {return buffer;}
    int texture() {return texture;}
    long capacity() {return capacity;}
    void allocate(long bytes) {
        if(bytes<=capacity)return;
        int write=GL11.glGetInteger(GL31.GL_COPY_WRITE_BUFFER);
        try {
            GL15.glBindBuffer(GL31.GL_COPY_WRITE_BUFFER,buffer);
            GL15.glBufferData(GL31.GL_COPY_WRITE_BUFFER,Math.max(32,bytes),GL15.GL_DYNAMIC_COPY);
            GL43.glClearBufferData(GL31.GL_COPY_WRITE_BUFFER,GL30.GL_R32UI,GL30.GL_RED_INTEGER,GL11.GL_UNSIGNED_INT,(IntBuffer)null);
            capacity=Math.max(32,bytes);attach();
        } finally {GL15.glBindBuffer(GL31.GL_COPY_WRITE_BUFFER,write);}
    }
    void upload(IntBuffer data) {
        int write=GL11.glGetInteger(GL31.GL_COPY_WRITE_BUFFER);
        try {
            GL15.glBindBuffer(GL31.GL_COPY_WRITE_BUFFER,buffer);
            GL15.glBufferData(GL31.GL_COPY_WRITE_BUFFER,data,GL15.GL_STREAM_DRAW);
            capacity=(long)data.remaining()*Integer.BYTES;attach();
        } finally {GL15.glBindBuffer(GL31.GL_COPY_WRITE_BUFFER,write);}
    }
    private void attach() {
        int previous=GL11.glGetInteger(GL31.GL_TEXTURE_BINDING_BUFFER);
        GL11.glBindTexture(GL31.GL_TEXTURE_BUFFER,texture);GL31.glTexBuffer(GL31.GL_TEXTURE_BUFFER,format,buffer);
        GL11.glBindTexture(GL31.GL_TEXTURE_BUFFER,previous);
    }
    @Override public void close() {GL11.glDeleteTextures(texture);GL15.glDeleteBuffers(buffer);}
}
