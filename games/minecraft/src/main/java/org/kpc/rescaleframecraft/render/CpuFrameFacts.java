package org.kpc.rescaleframecraft.render;

/** Engine identity across the post-Present GLFW poll and the following simulation. */
final class CpuFrameFacts {
    interface Sink { void fact(long source,int stage); }
    private final Sink sink;
    private long reserved;
    CpuFrameFacts(Sink sink) {this.sink=sink;}
    void simulation(long source) {
        // First activation can precede the first observed poll. Report the absence honestly.
        if(reserved!=source)sink.fact(source,NativeBridge.FRAME_BEGIN);
        sink.fact(source,NativeBridge.SIMULATION_BEGIN);
    }
    long beforeNextPoll(long rendered) {
        long source=rendered+1;
        if(reserved!=source) {
            reserved=source;
            sink.fact(source,NativeBridge.FRAME_BEGIN);
            sink.fact(source,NativeBridge.INPUT_SAMPLE);
        }
        return source;
    }
    void ended(long source) {sink.fact(source,NativeBridge.FRAME_END);}
    void reset() {reserved=0;}
}
