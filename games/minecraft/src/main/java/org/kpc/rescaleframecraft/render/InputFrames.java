package org.kpc.rescaleframecraft.render;

/** Associates sampled input with the simulation that can consume it. */
final class InputFrames {
    interface Sink { void input(long frame,int kinds); }
    private final Sink sink;
    private Thread owner;
    private long frame;
    private int pending;
    private boolean open;
    InputFrames(Sink sink) {this.sink=sink;}
    synchronized void begin(long id) {
        owner=Thread.currentThread();frame=id;open=true;
        if(pending!=0) {int kinds=pending;pending=0;sink.input(frame,kinds);}
    }
    synchronized void sample(int kinds) {
        if(open&&Thread.currentThread()==owner)sink.input(frame,kinds);
        else pending|=kinds;
    }
    synchronized void end() {open=false;}
}
