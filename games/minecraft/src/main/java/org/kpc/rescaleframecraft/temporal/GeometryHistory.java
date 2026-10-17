package org.kpc.rescaleframecraft.temporal;

import com.mojang.blaze3d.vertex.VertexConsumer;
import com.mojang.blaze3d.vertex.MeshData;
import com.mojang.blaze3d.vertex.VertexFormatElement;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.client.renderer.RenderType;

/** Retains actual CPU-transformed vertices, including animated model poses. */
public final class GeometryHistory {
    public static final int STRIDE=7;
    public static final class Batch {
        public final RenderType type;
        public float[] current=new float[1024], previous=new float[1024];
        public int size,previousSize;
        public long frame=-1,previousFrame=-1;
        Batch(RenderType type) { this.type=type; }
        void begin(long nextFrame) {
            if(frame==nextFrame)return;
            float[] old=previous;previous=current;current=old;
            previousSize=size;previousFrame=frame;size=0;frame=nextFrame;
        }
        int vertex(float x,float y,float z) {
            if(size+STRIDE>current.length)current=Arrays.copyOf(current,current.length*2);
            int index=size;size+=STRIDE;
            current[index]=x;current[index+1]=y;current[index+2]=z;
            current[index+3]=current[index+4]=0;current[index+5]=1;current[index+6]=0;
            return index;
        }
        public int vertices() { return size/STRIDE; }
        public boolean hasPrevious(long thisFrame) { return previousFrame==thisFrame-1&&previousSize==size; }
    }
    private final IdentityHashMap<Object,Map<RenderType,Batch>> actors=new IdentityHashMap<>();
    private final ArrayDeque<Object> owners=new ArrayDeque<>();
    private final List<Batch> draws=new ArrayList<>();
    private long frame;
    private boolean capturing;

    public void begin(long frame) { this.frame=frame;owners.clear();draws.clear();capturing=true; }
    public void push(Object actor) { if(capturing)owners.push(actor); }
    public void pop() { if(capturing&&!owners.isEmpty())owners.pop(); }
    public List<Batch> finish() {
        capturing=false;owners.clear();
        actors.values().removeIf(map->{map.values().removeIf(batch->batch.frame<frame-2);return map.isEmpty();});
        return draws;
    }
    public void clear() { actors.clear();owners.clear();draws.clear();capturing=false; }
    public void capture(Object owner,RenderType type,MeshData mesh) {
        if(!capturing)return;
        var state=mesh.drawState();var format=state.format();
        int pos=format.getOffset(VertexFormatElement.POSITION),uv=format.getOffset(VertexFormatElement.UV0),color=format.getOffset(VertexFormatElement.COLOR);
        if(pos<0)return;
        var materials=actors.computeIfAbsent(owner,key->new IdentityHashMap<>());
        var batch=materials.computeIfAbsent(type,Batch::new);
        if(batch.frame!=frame) { batch.begin(frame);draws.add(batch); }
        var bytes=mesh.vertexBuffer();
        for(int v=0;v<state.vertexCount();v++) {
            int base=v*format.getVertexSize();
            int i=batch.vertex(bytes.getFloat(base+pos),bytes.getFloat(base+pos+4),bytes.getFloat(base+pos+8));
            if(uv>=0) { batch.current[i+3]=bytes.getFloat(base+uv);batch.current[i+4]=bytes.getFloat(base+uv+4); }
            if(color>=0) {
                batch.current[i+5]=Byte.toUnsignedInt(bytes.get(base+color+3))/255f;
                batch.current[i+6]=Float.intBitsToFloat((Byte.toUnsignedInt(bytes.get(base+color))<<16)|(Byte.toUnsignedInt(bytes.get(base+color+1))<<8)|Byte.toUnsignedInt(bytes.get(base+color+2)));
            }
        }
    }
    public VertexConsumer wrap(VertexConsumer delegate,RenderType type) {
        if(!capturing||owners.isEmpty()||delegate instanceof CapturingConsumer)return delegate;
        Map<RenderType,Batch> materials=actors.computeIfAbsent(owners.peek(),key->new IdentityHashMap<>());
        Batch batch=materials.computeIfAbsent(type,Batch::new);
        if(batch.frame!=frame) { batch.begin(frame);draws.add(batch); }
        return new CapturingConsumer(delegate,batch);
    }
    private static final class CapturingConsumer implements VertexConsumer {
        private final VertexConsumer delegate;
        private final Batch batch;
        private int vertex=-1;
        CapturingConsumer(VertexConsumer delegate,Batch batch) { this.delegate=delegate;this.batch=batch; }
        @Override public VertexConsumer addVertex(float x,float y,float z) {
            delegate.addVertex(x,y,z);vertex=batch.vertex(x,y,z);return this;
        }
        @Override public VertexConsumer setColor(int r,int g,int b,int a) {
            delegate.setColor(r,g,b,a);
            if(vertex>=0) { batch.current[vertex+5]=a/255f;batch.current[vertex+6]=Float.intBitsToFloat((r<<16)|(g<<8)|b); }
            return this;
        }
        @Override public VertexConsumer setUv(float u,float v) {
            delegate.setUv(u,v);if(vertex>=0) { batch.current[vertex+3]=u;batch.current[vertex+4]=v; }return this;
        }
        @Override public VertexConsumer setUv1(int u,int v) { delegate.setUv1(u,v);return this; }
        @Override public VertexConsumer setUv2(int u,int v) { delegate.setUv2(u,v);return this; }
        @Override public VertexConsumer setNormal(float x,float y,float z) { delegate.setNormal(x,y,z);return this; }
    }
}
