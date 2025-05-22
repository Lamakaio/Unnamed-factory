
fn map.make_job(name) {
    if "job" !in this {
        this.job = #{};
    }
    this.job[name] = #{};
    this.job[name].population = 0.0;
    this.job[name].productivity = 0.0;
    this.job[name].happiness = 0.0;
    this.job[name].commute = 0.0;
}


data.make_job("Researcher");
data.make_job("Crafter");
data.make_job("Teacher");
data.make_job("Builder");
data.make_job("Collecter");
data.make_job("Artist");


data.stat = #{};

data.stat.fame = 0.0;
data.stat.migration = 0.0;
data.stat.natality = 0.0;
data.stat.science = 0.0;

data.resource = #{};
data.resource.money = 0.0;
data.resource.food = 0.0;
data.resource.materials = 0.0;

