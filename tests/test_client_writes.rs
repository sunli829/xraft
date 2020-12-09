mod test_raft;

use test_raft::TestHarness;

#[tokio::test]
async fn client_writes() {
    let test = TestHarness::default();
    test.add_node(1);
    test.add_node(2);
    test.add_node(3);
    test.initialize().await.unwrap();
}
