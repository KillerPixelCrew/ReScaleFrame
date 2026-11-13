package org.kpc.rescaleframecraft.render;

import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class CpuFrameFactsTest {
    @Test void postPresentPollBelongsToFollowingSimulation() {
        var seen=new ArrayList<String>();
        var facts=new CpuFrameFacts((id,stage)->seen.add(id+":"+stage));
        facts.simulation(17); // activation has no observed input poll
        facts.ended(17);
        assertEquals(18,facts.beforeNextPoll(17));
        assertEquals(18,facts.beforeNextPoll(17)); // recursive display cannot reserve twice
        facts.simulation(18);facts.ended(18);
        assertEquals(List.of("17:0","17:2","17:4","18:0","18:1","18:2","18:4"),seen);
        facts.reset();seen.clear();facts.simulation(19);
        assertEquals(List.of("19:0","19:2"),seen); // reset does not fabricate a poll
    }
}
