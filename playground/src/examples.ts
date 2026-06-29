// The playground examples gallery (in-browser track, slice 5a). Each is a small,
// **runnable** in-process (Bundle) program with a zero-argument `on call()` entry, so
// picking one and pressing Run produces output. Every example here is verified to
// compile and run in the in-process subset (the gallery never offers a snippet that
// would error on Run). The existing `examples/` apps are full Workers/agent programs
// outside that subset, so these are playground-specific teaching snippets.

export interface Example {
  id: string;
  title: string;
  source: string;
}

export const EXAMPLES: Example[] = [
  {
    id: "hello",
    title: "Hello, world",
    source: `context hello.world

consumes bynk { Logger }

service greet {
  on call() -> Effect[String] given Logger {
    let _ <- Logger.info("Hello from Bynk!")
    "Hello from Bynk!"
  }
}
`,
  },
  {
    id: "dice",
    title: "Capabilities — Random & Logger",
    source: `context demo.dice

consumes bynk { Random, Logger }

service roll {
  on call() -> Effect[Int] given Random, Logger {
    let _ <- Logger.info("Rolling a d6...")
    let n <- Random.int(1, 7)
    n
  }
}
`,
  },
  {
    id: "records",
    title: "Records",
    source: `context demo.records

consumes bynk { Logger }

type Point = { x: Int, y: Int }

service make {
  on call() -> Effect[Point] given Logger {
    let _ <- Logger.info("Building a point")
    Point { x: 3, y: 4 }
  }
}
`,
  },
  {
    id: "lights",
    title: "Sum types & match",
    source: `context demo.lights

consumes bynk { Logger }

type Light = enum { Red, Green, Yellow }

service next {
  on call() -> Effect[String] given Logger {
    let _ <- Logger.info("The light is green")
    match Green {
      Red => "stop",
      Green => "go",
      Yellow => "slow down",
    }
  }
}
`,
  },
  {
    id: "slug",
    title: "Refined types",
    source: `context demo.slug

consumes bynk { Logger }

type Slug = String where Matches("[a-z]+")

service check {
  on call() -> Effect[Bool] given Logger {
    let _ <- Logger.info("Is \\"hello\\" a valid slug?")
    match Slug.of("hello") {
      Ok(_) => true,
      Err(_) => false,
    }
  }
}
`,
  },
];
