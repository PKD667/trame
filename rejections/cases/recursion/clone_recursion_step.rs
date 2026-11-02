// spec: Requirement (Step): no recursion. Node::clone calls Vec<Node>::clone, which calls Node::clone. The clone and the
// drop of the old tree are allocation too (backend.rs M4); the cases above and below it are about recursion.
//@error-in-other-file: [allocation] step
#[trame::message]
struct M {
    n: u32,
}

struct Node {
    kids: Vec<Node>,
}

impl Clone for Node {
    fn clone(&self) -> Node {
        Node { kids: self.kids.clone() }
        //~^ ERROR: [recursion] step
        //~| ERROR: [allocation] step
    }
}

struct Q {
    tree: Node,
}

#[trame::state(steps(step))]
struct S {
    q: Q,
}

#[trame::step(S, receives = M)]
fn step(q: &mut Q) -> trame::Status {
    let t = q.tree.clone();
    q.tree = t;
    trame::Status::Done
}

fn main() {}
