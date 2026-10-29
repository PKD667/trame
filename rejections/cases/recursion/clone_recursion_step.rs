// spec: Requirement (Step): no recursion. Node::clone calls Vec<Node>::clone, which calls Node::clone.
#[trame::message]
struct M {
    n: u32,
}

struct Node {
    kids: Vec<Node>,
}

impl Clone for Node {
    fn clone(&self) -> Node {
        Node { kids: self.kids.clone() } //~ ERROR: [recursion] step
    }
}

struct Q {
    tree: Node,
}

struct P;

#[trame::process(receives = M)]
impl P {
    fn step(&mut self, q: &mut Q) -> trame::Status {
        let t = q.tree.clone();
        q.tree = t;
        trame::Status::Done
    }
}

fn main() {
    let mut q = Q { tree: Node { kids: Vec::new() } };
    let _ = trame::concurrent!(P; q);
}
