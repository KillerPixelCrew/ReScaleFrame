package org.kpc.rescaleframecraft.render;

/** Associates sampled input with the simulation that can consume it. */
final class InputFrames {
    interface Sink { void input(long frame,int kinds); }
    private final Sink sink;
    private Thread owner;
    private long frame;
    private boolean open;
    InputFrames(Sink sink) {this.sink=sink;}
    synchronized void begin(long id) {
        owner=Thread.currentThread();frame=id;open=true;
    }
    synchronized void sample(int kinds) {
        if(open&&Thread.currentThread()==owner)sink.input(frame,kinds);
        else sink.input(0,kinds); // the core owns bounded association and expiry
    }
    synchronized void end() {open=false;}
}
