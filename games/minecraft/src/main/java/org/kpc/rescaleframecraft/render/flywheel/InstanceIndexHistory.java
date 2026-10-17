package org.kpc.rescaleframecraft.render.flywheel;

import java.nio.IntBuffer;
import java.util.IdentityHashMap;
import org.lwjgl.BufferUtils;

/** Matches actual instance identities across compaction, page migration and slot reuse. Render-owner only. */
final class InstanceIndexHistory {
    private static final class Entry {
        long frame=-1;
        Object model;
        int base,count,previousBase;
    }
    private final IdentityHashMap<Object,Entry> instances=new IdentityHashMap<>();
    private IntBuffer indices=BufferUtils.createIntBuffer(2);
    private long frame=-1;
    private int slots;

    void begin(long frame,int slots) {
        this.frame=frame;this.slots=slots;
        int size=Math.multiplyExact(slots,2);
        if(indices.capacity()<size)indices=BufferUtils.createIntBuffer(Math.max(size,indices.capacity()*2));
        indices.clear().limit(size);
        for(int i=0;i<size;i++)indices.put(i,0);
    }
    void record(Object instance,Object model,int currentIndex,int vertexBase,int vertexCount) {
        if(currentIndex<0||currentIndex>=slots)throw new IllegalArgumentException("Instance outside GPU storage");
        var entry=instances.computeIfAbsent(instance,key->new Entry());
        if(entry.frame!=frame) {
            boolean consecutive=entry.frame>=0&&entry.frame==frame-1&&entry.model==model&&entry.count==vertexCount;
            entry.previousBase=consecutive?entry.base+1:0;
            entry.frame=frame;
        }
        entry.base=vertexBase;entry.count=vertexCount;entry.model=model;
        indices.put(currentIndex*2,entry.previousBase);
        indices.put(currentIndex*2+1,vertexBase);
    }
    IntBuffer data() {return indices;}
    void end() {instances.values().removeIf(entry->entry.frame!=frame);}
    void clear() {instances.clear();frame=-1;}
}
