package org.kpc.rescaleframecraft.render.flywheel;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class InstanceIndexHistoryTest {
    @Test void compactionAndPageMigrationFollowObjectsRatherThanGpuSlots() {
        var history=new InstanceIndexHistory();var a=new Object();var b=new Object();var model=new Object();
        history.begin(10,96);history.record(a,model,35,100,40);history.record(b,model,81,200,40);history.end();
        history.begin(11,96);history.record(b,model,1,300,40);history.record(a,model,66,400,40);
        assertEquals(201,history.data().get(2));assertEquals(300,history.data().get(3));
        assertEquals(101,history.data().get(132));assertEquals(400,history.data().get(133));
        history.record(a,model,66,400,40);
        assertEquals(101,history.data().get(132),"Multiple mesh draws must retain the same previous frame");
    }
    @Test void replacementsMissedFramesAndResetsCannotInheritOldGeometry() {
        var history=new InstanceIndexHistory();var removed=new Object();var replacement=new Object();var model=new Object();
        history.begin(20,8);history.record(removed,model,3,0,4);history.end();
        history.begin(21,8);history.record(replacement,model,3,0,4);
        assertEquals(0,history.data().get(6),"Reused GPU slot has a new identity");history.end();
        history.begin(23,8);history.record(replacement,model,3,0,4);
        assertEquals(0,history.data().get(6),"Missed frame invalidates history");
        history.clear();history.begin(1,8);history.record(replacement,model,3,0,4);
        assertEquals(0,history.data().get(6));
    }
    @Test void modelAndTopologyChangesInvalidateCorrespondence() {
        var history=new InstanceIndexHistory();var instance=new Object();var model=new Object();
        history.begin(1,1);history.record(instance,model,0,0,4);history.end();
        history.begin(2,1);history.record(instance,model,0,0,8);history.end();
        assertEquals(0,history.data().get(0));
        history.begin(3,1);history.record(instance,new Object(),0,0,8);
        assertEquals(0,history.data().get(0));
    }
}
