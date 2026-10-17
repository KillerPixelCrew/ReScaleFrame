package org.kpc.rescaleframecraft.render;

import java.util.ArrayList;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class InputFramesTest {
    @Test void eventsAfterPresentBelongToNextSimulation() {
        var events=new ArrayList<String>();
        var frames=new InputFrames((id,kinds)->events.add(id+":"+kinds));
        frames.begin(17);frames.sample(4);frames.end();
        frames.sample(1);frames.sample(2);
        assertEquals(java.util.List.of("17:4"),events);
        frames.begin(18);
        assertEquals(java.util.List.of("17:4","18:3"),events);
    }
    @Test void workerCannotTagAnInFlightRenderToken() throws Exception {
        var events=new ArrayList<String>();
        var frames=new InputFrames((id,kinds)->events.add(id+":"+kinds));
        frames.begin(23);
        var worker=new Thread(()->frames.sample(4));worker.start();worker.join();
        assertTrue(events.isEmpty());
        frames.end();frames.begin(24);
        assertEquals(java.util.List.of("24:4"),events);
    }
}
